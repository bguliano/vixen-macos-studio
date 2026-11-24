# Connect agents to Vixen Studio

Use the bridge inside the installed app bundle, Contents/Helpers/vixen-studio-mcp. During development the admitted build produces target/build-mcp/Vixen.app/Contents/Helpers/vixen-studio-mcp; focused tests also build target/debug/vixen-studio-mcp.

Use absolute executable/profile paths. The profile must be writable and must not be the original archive. Prefer opening that profile in Studio first: agents then share its live sessions and undo. Without the app, the bridge takes the profile lock and runs muted headless, serving that session to later bridges and scripts on the profile (see Scripts). Unsaved headless edits disappear on disconnect: explicitly save first. Existing broken socket endpoints or profile locks fail closed; investigate rather than blindly deleting them.

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

## Scripts

A script making thousands of calls shares the agents' session instead of fighting them for the profile lock. Every bridge on a profile attaches to whoever serves it: the app when it has the profile open, otherwise the first headless bridge, which serves its session to the rest. Unsaved documents and sequence IDs are the same for all of them, so Claude Code and any number of scripts (up to eight connections) can work on one live profile at once without a daemon of their own. A bad request (over 1 MiB, an answer over 8 MiB, a tool that fails internally) gets an error reply and never drops the connection. More for agents: [MCP-AGENT-GUIDE.md](MCP-AGENT-GUIDE.md).

One call per process, from any language or shell:

    vixen-studio-mcp --profile /absolute/path/show-copy call sequence_create '{"lengthMs":30000}'
    echo '{"sequenceId":"..."}' | vixen-studio-mcp --profile /absolute/path/show-copy call sequence_status -

The result is the tool's JSON on stdout; a refusal exits 1 with the tool's message on stderr. When nothing serves the profile, the first call starts a background session (`--serve`), which stops after 10 minutes with no clients; save documents you want to keep before then.

Many calls on one connection, from Python (standard library only; copy `studio/python/vixen_studio.py` next to the script):

    from vixen_studio import Studio
    with Studio("/absolute/path/show-copy", helper="/absolute/path/vixen-studio-mcp") as studio:
        seq = studio.call("sequence_create", lengthMs=30000)
        studio.call("sequence_status", sequenceId=seq["sequenceId"])

It runs `vixen-studio-mcp --profile PATH --persist`, which speaks MCP on stdio like the plain bridge but starts the background session instead of hosting one itself, so the session outlives the script. Any MCP client can do the same. To keep a session running while scripts come and go, start it yourself: `vixen-studio-mcp --profile PATH --serve` (add `--idle SECONDS` to stop it after that long with no clients). The bridge keeps its connection alive while the script is idle; if the session itself went away, vixen_studio.py reconnects on its next call.

## First workflow

1. studio_status, display_elements, controller_list, preview_list, diagnostics_report.
2. music_analyze on a filename in the profile's Media folder; music_rhythm for a bar-by-bar table (downbeats, 16th-slot onsets, loudness, sections, repeats).
3. sequence_create with lengthMs and media; retain sequenceId.
4. effect_catalog, then effect_add at detected beats; marks_create or sequence_music_marks. For rhythm that shows on the timeline: sequence_onset_marks, then effects_on_marks on a band's collection (snapped to the beat grid, level from strength, colours cycling). For staccato stabs: effect_step walks, alternates, stacks or jumps segments of a group (or horizontal bands) one step per mark.
5. sequence_render, preview_frame, preview_sheet or preview_video, diagnostics_report; correct using shared undo/redo.
6. Confirm sequence_save/profile_save/sequence_export_fseq only when wanted.

No hardware is needed to author, render or export. Output permission defaults OFF and can only be enabled locally through Studio. Headless and VIXEN_MUTE=1 cannot send output even if permission is set. Real hardware tools are separate and require confirmation. Schedule drafts stay disabled for local review.

The server uses legacy MCP initialize negotiation (2024-11-05). It does not advertise modern tasks/cancellation/HTTP. Long analysis/render calls need a generous client request timeout. See MCP.md for explicit capability limits.
