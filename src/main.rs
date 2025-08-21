use std::io::Write;
use std::os::fd::AsRawFd;

use listenfd::ListenFd;

fn main() -> anyhow::Result<()> {
    let mut listenfd = ListenFd::from_env();
    let socket = listenfd.take_tcp_listener(0)?.unwrap();
    fastcgi::run_raw(
        |mut req| {
            write!(
                &mut req.stdout(),
                "Content-Type: text/plain\n\nHello, world!"
            )
            .unwrap_or(());
        },
        socket.as_raw_fd(),
    );
    Ok(())
}
