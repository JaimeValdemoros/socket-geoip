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
            let ip = req.param("ip").unwrap();
            let mut stdout = req.stdout();
            let _ = write!(stdout, "Content-Type: text/plain\n\n");
            let Ok(ip) = ip.parse::<std::net::IpAddr>() else {
                return;
            };
            let _ = write!(stdout, "ip: {ip}\n");
            if let Ok(Some(city)) = reader.lookup::<maxminddb::geoip2::City>(ip) {
                if let (Some(city), Some(country)) = (city.city, city.country) {
                    if let (Some(city_names), Some(country_names)) = (city.names, country.names) {
                        let _ = write!(
                            stdout,
                            "city: {}\ncountry: {}\n",
                            city_names["en"], country_names["en"]
                        );
                    }
                }
            }
        },
        socket.as_raw_fd(),
    );
    Ok(())
}
