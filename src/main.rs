use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use listenfd::ListenFd;

static TICKER: AtomicU64 = AtomicU64::new(0);

fn main() -> anyhow::Result<()> {
    let mut listenfd = ListenFd::from_env();
    let socket = listenfd.take_raw_fd(0)?.unwrap();
    let reader = Arc::new(maxminddb::Reader::open_readfile(
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
            let mut stdout = req.stdout();
            let _ = write!(stdout, "Content-Type: application/json\n\n");
            let Ok(addr) = addr.parse::<std::net::SocketAddr>() else {
                return;
            };
            let ip = addr.ip();
            let mut output = serde_json::json!({
                "ip": ip
            });
            if let Ok(Some(city)) = reader.lookup::<maxminddb::geoip2::City>(ip) {
                if let Ok(serde_json::Value::Object(obj)) = serde_json::to_value(&city) {
                    output.as_object_mut().map(|m| m.extend(obj));
                }
            }
            let _ = serde_json::to_writer_pretty(stdout, &output);
        },
        socket,
    );
    Ok(())
}
