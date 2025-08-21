use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use listenfd::ListenFd;

static TICKER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, serde::Serialize)]
struct Output<'a> {
    ip: std::net::IpAddr,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    data: Option<maxminddb::geoip2::City<'a>>,
}

fn main() -> anyhow::Result<()> {
    let mut listenfd = ListenFd::from_env();
    let socket = listenfd.take_raw_fd(0)?.unwrap();
    let reader = Arc::new(maxminddb::Reader::open_mmap(
        std::env::var("DB_FILE").unwrap(),
    )?);
    if let Ok(timeout_secs) = std::env::var("TIMEOUT_SECS") {
        let timeout = Duration::from_secs(timeout_secs.parse().unwrap());
        std::thread::spawn(move || {
            let mut last_ticker = 0;
            loop {
                std::thread::sleep(timeout);
                let new_ticker = TICKER.load(Ordering::Relaxed);
                if last_ticker == new_ticker {
                    eprintln!("idle, exiting");
                    eprintln!("{new_ticker} requests served");
                    std::process::exit(0);
                }
                last_ticker = new_ticker;
            }
        });
    };
    fastcgi::run_raw(
        move |mut req| {
            TICKER.fetch_add(1, Ordering::Relaxed);
            let addr = req.param("REMOTE").unwrap();
            let Ok(addr) = addr.parse::<std::net::SocketAddr>() else {
                return req.exit(1);
            };
            let ip = addr.ip();
            let output = Output {
                ip,
                data: reader.lookup(ip).unwrap_or(None),
            };
            let mut stdout = req.stdout();
            write!(stdout, "Content-Type: application/json\n\n").unwrap();
            serde_json::to_writer_pretty(stdout, &output).unwrap();
        },
        socket,
    );
    Ok(())
}
