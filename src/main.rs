use std::net::{IpAddr, SocketAddr};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use listenfd::ListenFd;
use maxminddb::{Mmap, Reader};
use tokio::{
    net::{TcpListener, TcpStream, tcp::WriteHalf},
    task::{LocalSet, spawn_blocking},
};
use tokio_fastcgi::{Request, RequestResult, Requests};
use tokio_util::future::FutureExt;
use tokio_util::sync::CancellationToken;

static TICKER: AtomicU64 = AtomicU64::new(0);
static READER: OnceLock<Reader<Mmap>> = OnceLock::new();

#[derive(Debug, serde::Serialize)]
struct Output<'a> {
    ip: std::net::IpAddr,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    data: Option<maxminddb::geoip2::City<'a>>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let mut listenfd = ListenFd::from_env();
    let socket = listenfd.take_tcp_listener(0)?.unwrap();
    socket.set_nonblocking(true)?;
    let socket = TcpListener::from_std(socket)?;

    if let Some(reader) = std::env::var("DB_FILE")
        .ok()
        .map(Reader::open_mmap)
        .transpose()?
    {
        READER.set(reader).expect("READER already set");
    };

    let local = LocalSet::new();

    let shutdown = CancellationToken::new();

    local.spawn_local({
        let shutdown = shutdown.clone();
        async move {
            tokio::select! {
                res = tokio::signal::ctrl_c() => {
                    res.expect("ctrl-c handler failed");
                    eprintln!("Received SIGINT signal, triggering shutdown");
                    shutdown.cancel();
                }
                () = shutdown.cancelled() => (),
            }
        }
    });

    if let Ok(timeout_secs) = std::env::var("TIMEOUT_SECS") {
        let shutdown = shutdown.clone();
        let timeout = Duration::from_secs(timeout_secs.parse().unwrap());
        local.spawn_local(async move {
            let mut last_ticker = 0;
            loop {
                tokio::time::sleep(timeout).await;
                let new_ticker = TICKER.load(Ordering::Relaxed);
                if last_ticker == new_ticker {
                    eprintln!("idle, exiting");
                    eprintln!("{new_ticker} requests served");
                    shutdown.cancel();
                    break;
                }
                last_ticker = new_ticker;
            }
        });
    };

    let res = local
        .run_until({
            let shutdown = shutdown.clone();
            async move {
                loop {
                    let connection = socket.accept().with_cancellation_token(&shutdown).await;
                    let (stream, addr) = match connection {
                        None => break Ok(()),
                        Some(Err(e)) => break Err(e),
                        Some(Ok(x)) => x,
                    };
                    eprintln!("New stream: {addr}");
                    let shutdown = shutdown.clone();
                    tokio::task::spawn_local(async move {
                        if let Err(e) = handle_stream(stream, addr, &shutdown).await {
                            eprintln!("{e:?}");
                        }
                    });
                }
            }
        })
        .await;

    // wait for remaining spawned tasks
    eprintln!("Waiting for remaining connections to complete");
    shutdown.cancel();
    local.await;
    eprintln!("Done waiting, shutting down");

    res.map_err(Into::into)
}

async fn handle_stream(
    mut stream: TcpStream,
    addr: SocketAddr,
    token: &CancellationToken,
) -> anyhow::Result<()> {
    let mut requests = Requests::from_split_socket(stream.split(), 10, 10);
    while let Some(Ok(Some(request))) = requests.next().with_cancellation_token(token).await {
        TICKER.fetch_add(1, Ordering::Relaxed);
        request
            .process(|request| async move {
                match handle_req(request).await {
                    Ok(()) => RequestResult::Complete(0),
                    Err(e) => {
                        eprintln!("{e}");
                        RequestResult::Complete(1)
                    }
                }
            })
            .await?;
    }
    eprintln!("Stream terminated: {addr}");
    anyhow::Ok(())
}

async fn handle_req(req: Arc<Request<WriteHalf<'_>>>) -> anyhow::Result<()> {
    let Some(addr) = req.get_str_param("REMOTE_ADDR") else {
        anyhow::bail!("No REMOTE_ADDR set");
    };
    let ip = addr.parse::<IpAddr>()?;
    let output = Output {
        ip,
        data: match READER.get() {
            Some(r) => spawn_blocking(move || r.lookup(ip)).await??,
            None => None,
        },
    };
    let mut stdout = req.get_stdout();
    stdout.write(b"Content-Type: application/json\n\n").await?;
    stdout
        .write(serde_json::to_vec_pretty(&output)?.as_slice())
        .await?;
    Ok(())
}
