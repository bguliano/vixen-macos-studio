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
- Plan a song by bar with `music_rhythm`: one row per bar (downbeat, tempo, the 16th-slot onset table per band as text,
  loudness, section, which earlier bar it repeats), plus onsets, an energy curve and estimated sections. Correct the
  sections by ear and write them with `sequence_section_marks` (by `bar` or `startMs`); `music_rhythm` then reports
  your names from the sequence's "Sections" marks. Downbeats, sections and repeats are estimates.
- For a group of effects that fires on every note or beat (a swoosh then a glow, a slam with a ring, stripes that step), use
  `motif_place` on a mark collection instead of building each hit yourself: pick a built-in (`motif_list`) or save your
  own recipe with `motif_save`, pass two `targetSets` to alternate left/right, and check it with `dryRun:true` first. It
  is one undo step.
- **Pick effects by their look first.** `effect_catalog` (and `effects_library`) give each effect a `looks` text: what it
  does on a megatree, matrix, arch or line in plain words. Two surprises it spells out: plain Vertical `Wipe` moves
  *downward* (set ReverseDirection to rise), and `Chase` and `Spin` on a megatree group light only a one-string sliver
  at a time unless DepthOfEffect is 1, which steps whole strings around the trunk.
- **Target by where lights sit**, not by guessing string numbers: `targetRegion` on `effect_add`, `effect_band`,
  `effects_batch` and `section_rebuild` ops takes `elementId` plus `angleDeg` (megatrees, 0 front, 90 viewer's right,
  `[315,45]` wraps through the front), `heightPct` (along the strings from the base), `xPct`/`yPct`, measured exactly as
  `display_geometry` reports them. Whole strings that match are one target each; for a height range on a whole tree,
  `effect_band` is one Wipe per string instead of one effect per light. One undo step per call.
- **Set the sequence's palette once** with `sequence_palette` (`colors`, optional `distance`). It is saved with the
  sequence and one undo step; every add or parameter edit that brings in a colour outside it returns
  `paletteWarnings`, and `render_analyze` checks against it when you don't pass `palette`.
- For soft-edged zones on a tree or matrix, use `effect_band` with `featherPct` (or `featherLights`, rows of lights)
  instead of tuning a parked Wipe by hand; add `exclusive:true` and the band cuts whatever was lighting its rows for its
  time (other rows and other props keep their effects), so stacked zones never overlap. One undo step.

- To rebuild one section, use `section_rebuild` with the section's `range` and a `buildId` (e.g. `"chorus-2 v3"`) instead of
  deleting and re-adding effects: it replaces only that range (optionally only `targetIds`), tags every new effect with the
  build id, trims effects crossing the edges, and is one undo step. It refuses when Braden has hand-edited the section
  since your build (moved, changed, added or deleted effects there) and lists the edits; read them with `build_edits`, then
  rebuild around them with `onHandEdits:"keep"`, or ask before using `"replace"`. Tag a sequence built before builds were
  tracked once with `build_tag` (`all:true`), and pass `buildId` to `effects_batch` for effects placed outside a rebuild.
- To port an old show (B79), start with `effects_port_from_sequence` `dryRun:true` and `autoZones:true` on the old `.tim`: it
  lists the zones (the old rig's channels grouped by name) to route. Take out the old rig's lag with `offsetMs` (e.g. -35),
  snap with `grid:"16th"` (from the song's bars, as `music_rhythm` reads them) and a `snapToleranceMs` so hand-placed
  syncopation isn't forced onto the grid, and read `moves.furthest` before trusting it. Then add `map` (zone → Studio
  targets) and run it for real with a `buildId`: one undo step. `marks_import_from_sequence` takes the same options when you
  only want the old timing as marks.
- Run `build_edits` at the start of a session on an existing show: it reports what changed in the GUI since your last build.
- Save milestones with a label (`sequence_save` with `label:"WiW v6"`): `sequence_versions` lists them and
  `sequence_version_restore` puts one back in place (one undo step).

## Checking

- Before showing anyone a build, run `render_analyze` on the sequence (`scope` your main prop, `marks` your beat
  collection, `palette` your colours). It reads the rendered schedule, so it takes seconds, and flags what viewers reject:
  dark stretches (a dark intro), spazzing (more than 6 whole-prop changes a second), changes on no mark, and sections that
  lean blue/purple or leave the palette. Fix what it flags, starting from `jumpTo`, then run it again until `ok` is true;
  watch a `preview_video` of the flagged times only when the report isn't enough. It changes nothing.
