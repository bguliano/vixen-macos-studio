# Connect agents to Vixen Studio

Use the bridge inside the installed app bundle, Contents/Helpers/vixen-studio-mcp. During development the admitted build produces target/build-mcp/Vixen.app/Contents/Helpers/vixen-studio-mcp; focused tests also build target/debug/vixen-studio-mcp.

Use absolute executable/profile paths. The profile must be writable and must not be the original archive. Prefer opening that profile in Studio first: agents then share its live sessions and undo. Without the app, the bridge takes the profile lock and runs muted headless. Unsaved headless edits disappear on disconnect: explicitly save first. Existing broken socket endpoints or profile locks fail closed; investigate rather than blindly deleting them.

## Claude Code

    claude mcp add --transport stdio vixen-studio -- /absolute/path/vixen-studio-mcp --profile /absolute/path/show-copy

Inspect tools/list or ask for studio_status and diagnostics_report before editing. Keep destructive/live tools behind the client's normal approval policy.

## Codex

In the Codex MCP configuration:

    [mcp_servers.vixen-studio]
    command = "/absolute/path/vixen-studio-mcp"
    args = ["--profile", "/absolute/path/show-copy"]

## OpenClaw

Control UI: Settings -> MCP -> Add server -> Stdio. Name: vixen-studio. Command: the absolute helper path. Arguments: --profile followed by the absolute profile path. Save, then probe reachability; saving a definition alone is not verification.

Equivalent supported CLI (OpenClaw MCP documentation checked 2026-11-18):

    openclaw mcp add vixen-studio --command /absolute/path/vixen-studio-mcp --arg=--profile --arg /absolute/path/show-copy
    openclaw mcp doctor vixen-studio --probe

Definitions live under mcp.servers; tool-profile/session policy still applies. A client-side read-only policy is appropriate for inspection-only use. No OpenClaw/Claude/Codex configuration is modified by building this project.

## First workflow

1. studio_status, display_elements, controller_list, preview_list, diagnostics_report.
2. music_analyze on a filename in the profile's Media folder.
3. sequence_create with lengthMs and media; retain sequenceId.
4. effect_catalog, then effect_add at detected beats; marks_create or sequence_music_marks.
5. sequence_render, preview_frame, diagnostics_report; correct using shared undo/redo.
6. Confirm sequence_save/profile_save/sequence_export_fseq only when wanted.

No hardware is needed to author, render or export. Output permission defaults OFF and can only be enabled locally through Studio. Headless and VIXEN_MUTE=1 cannot send output even if permission is set. Real hardware tools are separate and require confirmation. Schedule drafts stay disabled for local review.

The server uses legacy MCP initialize negotiation (2024-11-05). It does not advertise modern tasks/cancellation/HTTP. Long analysis/render calls need a generous client request timeout. See MCP.md for explicit capability limits.
