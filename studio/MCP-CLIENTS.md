# Connecting MCP clients (read-only milestone)

Build/test: source ./env.sh; VIXEN_MUTE=1 CARGO_BUILD_JOBS=2 cargo test -p studio-mcp
The focused test builds target/debug/vixen-studio-mcp. Use absolute executable and copied-profile paths.

Claude Code: claude mcp add --transport stdio vixen-studio -- /absolute/path/vixen-studio-mcp --profile /absolute/path/show-copy

Codex configuration:

    [mcp_servers.vixen-studio]
    command = "/absolute/path/vixen-studio-mcp"
    args = ["--profile", "/absolute/path/show-copy"]

For OpenClaw, register a local stdio MCP server using its installed MCP integration/configuration UI, with the same command and arguments. No OpenClaw configuration is modified here; configuration keys depend on the installed integration.

The bridge acquires the existing Vixen profile lock and serves a **read-only snapshot**, not live app state. Close the app before connecting; lock conflicts fail closed. It does not launch audio, controllers, app services or rendering. Seven tools, a contract resource and a planning prompt are available. Writes, socket attachment, live notifications, music analysis and exports are not yet implemented. Investigate a lock left after abnormal exit before manually removing it.
