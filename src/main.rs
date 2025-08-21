use std::io::Write;
use std::os::fd::AsRawFd;
use std::sync::Arc;

use listenfd::ListenFd;

fn main() -> anyhow::Result<()> {
    let mut listenfd = ListenFd::from_env();
    let socket = listenfd.take_tcp_listener(0)?.unwrap();
    let reader = Arc::new(maxminddb::Reader::open_readfile(
        std::env::var("DB_FILE").unwrap(),
    )?);
    fastcgi::run_raw(
        move |mut req| {
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
        socket.as_raw_fd(),
    );
    Ok(())
}
