# Agent guide: working with Vixen Studio over MCP

Short, practical notes for agents that build shows through the `vixen-studio-mcp` bridge. Setup is in
[MCP-CLIENTS.md](MCP-CLIENTS.md), every tool in [MCP-TOOLS.md](MCP-TOOLS.md), the server's design in [MCP.md](MCP.md).

## Connecting

- **Several clients share one profile; no daemon of your own is needed.** Every bridge on a profile attaches to whoever
  serves it: the app when the profile is open in it, otherwise the first headless bridge, or a session started with
  `vixen-studio-mcp --profile PATH --serve`. Claude Code, your scripts (`--persist`, `call`, `studio/python/vixen_studio.py`)
  and other agents then see the same documents, sequence IDs and undo. Only the process serving the profile holds its lock.
- To keep one session running for a whole build while clients come and go, start `--serve` yourself (no idle limit), or
  point clients at `--persist`, which starts a background session that stops after 10 minutes with no clients.
- Up to eight connections at once. A ninth is refused with a message saying so; a connection idle for 120 seconds frees
  its slot, but a bridge's own keepalive keeps a quiet MCP client connected for as long as the client runs.
- Nothing a single request does ends the connection: a request over 1 MiB, a response over 8 MiB or a tool that fails
  internally gets a JSON-RPC error and the connection carries on. Pass a big batch as `opsFile`; page large lists with
  `limit`. An internal error is logged to the profile log; read the state back before retrying.

## Building

- Read `display://summary` once at the start of a session for every prop's geometry.
- Save explicitly (`sequence_save`); unsaved headless documents end with the session.
- For a group of effects that fires on every note or beat (a swoosh then a glow, a slam with a ring, stripes that step), use
  `motif_place` on a mark collection instead of building each hit yourself: pick a built-in (`motif_list`) or save your
  own recipe with `motif_save`, pass two `targetSets` to alternate left/right, and check it with `dryRun:true` first. It
  is one undo step.
- For soft-edged zones on a tree or matrix, use `effect_band` with `featherPct` (or `featherLights`, rows of lights)
  instead of tuning a parked Wipe by hand; add `exclusive:true` and the band cuts whatever was lighting its rows for its
  time (other rows and other props keep their effects), so stacked zones never overlap. One undo step.

## Checking

- Before showing anyone a build, run `render_analyze` on the sequence (`scope` your main prop, `marks` your beat
  collection, `palette` your colours). It reads the rendered schedule, so it takes seconds, and flags what viewers reject:
  dark stretches (a dark intro), spazzing (more than 6 whole-prop changes a second), changes on no mark, and sections that
  lean blue/purple or leave the palette. Fix what it flags, starting from `jumpTo`, then run it again until `ok` is true;
  watch a `preview_video` of the flagged times only when the report isn't enough. It changes nothing.
