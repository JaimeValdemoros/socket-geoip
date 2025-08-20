use listenfd::ListenFd;

fn main() -> anyhow::Result<()> {
    let mut listenfd = ListenFd::from_env();
    let _socket = listenfd.take_tcp_listener(0)?;
    Ok(())
}
