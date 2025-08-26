use std::cell::Cell;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_compat::CompatExt;
use futures::StreamExt;
use listenfd::ListenFd;
use maxminddb::{Mmap, Reader};
use tokio_fastcgi::{Request, RequestResult, Requests};

mod cancel;
mod timeout;

use cancel::FutureExt as _;
use timeout::FutureExt as _;

static READER: OnceLock<Reader<Mmap>> = OnceLock::new();

#[derive(Debug, serde::Serialize)]
struct Output<'a> {
    ip: std::net::IpAddr,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    data: Option<maxminddb::geoip2::City<'a>>,
}

fn main() -> anyhow::Result<()> {
    smol::block_on(async_main())
}

async fn async_main() -> anyhow::Result<()> {
    let mut listenfd = ListenFd::from_env();
    let socket = listenfd.take_tcp_listener(0)?.unwrap();
    let socket = smol::net::TcpListener::from(smol::Async::new(socket)?);

    if let Some(reader) = std::env::var("DB_FILE")
        .ok()
        .map(Reader::open_mmap)
        .transpose()?
    {
        READER.set(reader).expect("READER already set");
    };

    let (token, shutdown) = cancel::Token::new();
    let shutdown = &shutdown;
    let ticker = &Cell::new(0);

    let local = smol::LocalExecutor::new();

    let signal = async_ctrlc::CtrlC::new()?;
    let sigint_task = local.spawn(async {
        signal.await;
        shutdown();
    });

    let mut timeout_task = None;
    if let Ok(timeout_secs) = std::env::var("TIMEOUT_SECS") {
        let timeout = Duration::from_secs(timeout_secs.parse().unwrap());
        let task = local.spawn(async move {
            let mut timer = smol::Timer::interval(timeout);
            let mut last_ticker = 0;
            loop {
                timer.next().await;
                let new_ticker = ticker.get();
                if last_ticker == new_ticker {
                    eprintln!("idle, exiting");
                    eprintln!("{new_ticker} requests served");
                    shutdown();
                    break;
                }
                last_ticker = new_ticker;
            }
        });
        timeout_task = Some(task);
    };

    let res = local
        .run({
            async {
                loop {
                    let connection = socket.accept().with_cancel(&token).await;
                    let (stream, addr) = match connection {
                        None => break Ok(()),
                        Some(Err(e)) => break Err(e),
                        Some(Ok(x)) => x,
                    };
                    eprintln!("New stream: {addr}");
                    let token = token.clone();
                    local
                        .spawn(async move {
                            if let Err(e) = handle_stream(stream, addr, &token, &ticker).await {
                                eprintln!("{e:?}");
                            }
                        })
                        .detach();
                }
            }
        })
        .await;

    // Cancel background tasks
    eprintln!("Cancelling background tasks");
    local
        .run(async {
            sigint_task.cancel().await;
            if let Some(timeout_task) = timeout_task {
                timeout_task.cancel().await;
            }
        })
        .await;

    // wait for remaining spawned tasks
    eprintln!("Waiting for remaining connections to complete");
    shutdown();
    let timed_out = (async {
        while !local.is_empty() {
            local.tick().await;
        }
    })
    .timeout(Duration::from_secs(20))
    .await
    .is_none();

    if timed_out {
        eprintln!("Timed out waiting for connections, exiting");
    } else {
        eprintln!("Done waiting, shutting down");
    }

    res.map_err(Into::into)
}

async fn handle_stream(
    stream: smol::net::TcpStream,
    addr: SocketAddr,
    token: &cancel::Token,
    ticker: &Cell<u64>,
) -> anyhow::Result<()> {
    let (read, write) = smol::io::split(stream);
    let mut requests = Requests::new(read.compat(), write.compat(), 10, 10);
    while let Some(Ok(Some(request))) = requests.next().with_cancel(&token).await {
        ticker.update(|x| x + 1);
        request
            .process(|request| async move {
                match handle_req(request).timeout(Duration::from_secs(10)).await {
                    Some(Ok(())) => RequestResult::Complete(0),
                    Some(Err(e)) => {
                        eprintln!("{e}");
                        RequestResult::Complete(1)
                    }
                    None => RequestResult::Overloaded,
                }
            })
            .await?;
    }
    eprintln!("Stream terminated: {addr}");
    anyhow::Ok(())
}

async fn handle_req(req: Arc<Request<impl tokio::io::AsyncWrite + Unpin>>) -> anyhow::Result<()> {
    let Some(addr) = req.get_str_param("REMOTE_ADDR") else {
        anyhow::bail!("No REMOTE_ADDR set");
    };
    let ip = addr.parse::<IpAddr>()?;
    let output = Output {
        ip,
        data: match READER.get() {
            Some(r) => smol::unblock(move || r.lookup(ip)).await?,
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
