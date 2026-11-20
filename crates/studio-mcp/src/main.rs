use std::path::PathBuf;
use studio_mcp::lock::{self, Note};
fn main() {
    if let Err(error) = run() {
        // Every failing exit says why, on stderr and in the Vixen logs.
        lock::note(Note::Error, &error.to_string());
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    // First, before any thread starts: a stop signal removes a held lock instead of leaving it behind.
    lock::handle_stop_signals()?;
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
    // A refused endpoint is a dead app's leftover only if nothing holds the profile: take the lock first (it fails if a live
    // process holds it; a dead process's lock is cleared by lock::take), then remove the endpoint and serve headless.
    let connected = match studio_mcp::socket::connect(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            let held = lock::hold(lock::take(&root)
                .map_err(|_| "MCP endpoint is refusing connections and the profile is locked by a running process; open the app, or remove the stale socket")?);
            let path = studio_mcp::socket::socket_path(&root)?;
            let recovered = studio_mcp::socket::recover_stale(&path, studio_mcp::socket::lock_held_here(&root));
            drop(held);
            if !recovered? {
                return Err("MCP endpoint is refusing connections; it was not removed".into());
            }
            studio_mcp::socket::connect(&root)?
        }
        other => other?,
    };
    if let Some(mut socket) = connected {
        let mut reader = socket.try_clone()?;
        let inbound =
            std::thread::spawn(move || std::io::copy(&mut reader, &mut std::io::stdout().lock()));
        std::io::copy(&mut std::io::stdin().lock(), &mut socket)?;
        socket.shutdown(std::net::Shutdown::Write)?;
        inbound.join().map_err(|_| "socket reader failed")??;
        return Ok(());
    }
    // Never steal a running process's profile; only a provably dead owner's lock is cleared. The guard removes the lock on return,
    // including stdin EOF, and the signal thread removes it on SIGTERM, SIGINT or SIGHUP.
    let _held = lock::hold(lock::take(&root)?);
    let mut server = studio_mcp::Server::open(&root)?;
    studio_mcp::serve(
        &mut server,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )?;
    Ok(())
}
