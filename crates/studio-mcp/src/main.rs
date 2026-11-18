use std::path::PathBuf;
fn main() {
    if let Err(error) = run() {
        eprintln!("Vixen Studio MCP: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 || args[0] != "--profile" {
        return Err("usage: vixen-studio-mcp --profile PATH".into());
    }
    let root = PathBuf::from(&args[1]).canonicalize()?;
    if root
        .components()
        .any(|c| c.as_os_str() == "vixen-show-original")
    {
        return Err("Use a show copy, not the original archive".into());
    }
    if let Some(mut socket) = studio_mcp::socket::connect(&root)? {
        let mut reader = socket.try_clone()?;
        let inbound =
            std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::stdout().lock()));
        std::io::copy(&mut std::io::stdin().lock(), &mut socket)?;
        socket.shutdown(std::net::Shutdown::Write)?;
        inbound.join().map_err(|_| "socket reader failed")??;
        return Ok(());
    }
    // Fail closed even for stale locks; never steal a running app's profile.
    let lock = vixen_io::lock::create_lock_file(&root)?;
    struct Guard(PathBuf);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = vixen_io::lock::try_remove_lock_file(&self.0);
        }
    }
    let _guard = Guard(lock);
    let mut server = studio_mcp::Server::open(&root)?;
    studio_mcp::serve(
        &mut server,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )?;
    Ok(())
}
