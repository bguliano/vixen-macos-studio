# Agent guide: building shows with Vixen Studio over MCP

How to build a show through the `vixen-studio-mcp` bridge, from an empty sequence to a finished, checked show. Setup is
in [MCP-CLIENTS.md](MCP-CLIENTS.md), every tool in [MCP-TOOLS.md](MCP-TOOLS.md), the server's design in [MCP.md](MCP.md),
and what was learned building a whole show this way in [MCP-AGENT-GAPS.md](MCP-AGENT-GAPS.md).

The low-level tools (`effect_add`, `effects_batch`, `effect_set_parameter`...) are always there; the tools below are
compositions of them with predictable results. Every edit is one undo step (`sequence_undo`), most take `dryRun:true`,
and none of them restricts what you can still do by hand.

## The workflow

1. **Learn the display: `display_layout`.** One call: every prop in the main preview with the element id to target,
   `channels` and `control` (a "one channel" prop is a single string that only changes level and colour as a whole:
   Wipes, Chases, Meteors and Spins show nothing on it), `colors` (`"full"` for RGB, otherwise the only colours it can
   show: a red effect on a white-only string is dark), where it sits (centre, box and area, 0–100 of the display),
   `order` (left to right, inside out, bottom to top) and `mirrorPairs`. For one prop's lights, strings and angles use
   `display_geometry` (megatrees, matrices, arches) or the `display://summary` resource.
2. **Make the sequence: `library_new_from_audio`** with the song's path, then `sequence_open` its file.
3. **Plan by bar: `music_rhythm`** with the `sequenceId`: tempo, one row per bar (downbeat, the 16th-slot onset table per
   band, loudness, which earlier bar it repeats) and estimated sections. **`sequence_section_marks`** with no sections
   writes the estimate as the "Sections" marks; correct names and starts by ear and write them again (`sections` by
   `bar` or `startMs`). From here on, tools take a section name.
4. **Beat and hit marks:** `sequence_music_marks` (beats as "Music - All", bars as "Music 1/4 Beat #1 (Bar)") and, for
   rhythm that shows on the timeline, `sequence_onset_marks` (low, mid and high hits with strengths).
5. **Palette: `sequence_palette`** with the show's colours. For fixed-colour props use the colours `display_layout` lists.
   Adds and edits then report `paletteWarnings`, and `render_analyze` checks against it.
6. **Build one example of each section**, by section name:
   - effects on every beat or hit: `effects_on_marks` (`section:"Verse 1"`, `rotateTargets:true` to alternate props);
   - a group of effects per hit (slams, swooshes, marching stripes, a chase across props): `motif_place` with
     `targetSets` from `display_layout`'s `order` (e.g. `leftToRight`), `section`, and `dryRun:true` first;
   - staccato stabs: `effect_step`; still zones on a tree or matrix: `effect_band` (`featherPct`, `exclusive`);
   - long washes, twinkles and fades: `effects_batch` adds, with the section's `startMs`/`endMs` from `sequence_summary`.
7. **Repeat it: `passage_copy`.** `{section:"Chorus 1", toSections:"Chorus*", repeat:true}` puts the chorus on every
   other chorus, laid end to end until each is full; `targetMap` retargets on the way (`mirrorPairs` gives left/right
   twins), `replace:true` clears what was there. Copies keep every parameter.
8. **Review: `sequence_summary`** with `previewId`: per section the effects by type and the props used, and the
   problems: empty sections, effects past the song's end, duplicates, props that stay dark in a section they have
   effects in, props nothing lights. Fix and read it again until `ok`.
9. **Adjust in bulk: `effects_find` / `effects_edit`.** Same filter for both: `range` or `section` (`"Chorus*"`),
   `types`, `targetIds`, `fromBuild`, `layerId`. `effects_edit` sets `params`, `color`, `levelFromEnergy`, `levelScale`,
   `shiftMs`, `stretch`, or deletes (`delete:true, confirm:true`). `levelFromEnergy` (`{range:[20,100]}`) shapes each
   effect's level curve from the song's loudness over its span: a build-up or swell that follows the music.
10. **Taste check: `render_analyze`** (`marks:["Music - All"]`, `sections:"Sections"`) flags dark stretches, spazzing,
    changes off the beat and colours that lean blue/purple or leave the palette, each with a time to jump to. Watch only
    what it flags: `preview_section_sheet` (key moments of every section in one image), `preview_sheet` (a contact
    sheet of a span) or `preview_video`.
11. **Save: `sequence_save`** with `confirm:true`, and a `label` for milestones (`sequence_versions`,
    `sequence_version_restore`).

## Which tool for which job

| Job | Tool |
|---|---|
| What props are there, where, how controllable, which colours | `display_layout` |
| Where each light of a prop is (strings, height, angle) | `display_geometry` |
| Song structure, bars, tempo, loudness, repeats | `music_rhythm`, then `sequence_section_marks` |
| Beat, bar and hit marks | `sequence_music_marks`, `sequence_onset_marks` |
| One effect on every mark | `effects_on_marks` |
| A multi-effect figure on every mark, chases across props | `motif_place` (`motif_list`, `motif_save`) |
| Staccato segment walks | `effect_step` |
| Bands and zones by height | `effect_band`; `targetRegion` on adds for angle/height |
| Many explicit edits at once | `effects_batch` (`opsFile` for big ones) |
| Repeat or mirror a passage | `passage_copy` |
| Find effects by section, type, prop, build | `effects_find` |
| Change many effects at once | `effects_edit` |
| Brightness that follows the song's loudness | `effects_edit` `levelFromEnergy` |
| What is on at a moment | `effects_find` with `range:[t, t+1]`, `match:"overlaps"` |
| Rebuild one section without losing hand edits | `section_rebuild`, `build_edits`, `build_tag` |
| The whole sequence in one read, with problems | `sequence_summary` |
| Taste checks on the rendered show | `render_analyze`, `sequence_activity` |
| See it | `preview_section_sheet`, `preview_sheet`, `preview_video`, `preview_frame`, `effect_gallery` |
| Port an old show | `effects_port_from_sequence`, `marks_import_from_sequence` |

## Worked example: a 3-minute show in 23 calls

*Wizards in Winter* on a display of ten one-channel relay strings (each in one fixed colour) and two pixel strips (the
walkthrough in [MCP-AGENT-GAPS.md](MCP-AGENT-GAPS.md); before these tools it took 532 calls and still failed).

1. `display_layout` → ten props are "one channel" with fixed colours (mini trees white, tiers red, bulbs green, star
   blue); `order.leftToRight` is Mini Tree L, Light Strip L, the centre tree, Light Strip R, Mini Tree R;
   `mirrorPairs` pairs the mini trees and the strips.
2. `library_new_from_audio`, `sequence_open`, `music_rhythm`, `sequence_section_marks` (Intro, Verse 1–4, Chorus 1–3,
   Bridge 1–2), `sequence_music_marks`, `sequence_palette` (the props' own colours).
3. `sequence_summary` once, for the section times. Intro: one `effects_batch` (twinkles on the strips, a red fade-up
   on the tiers, the star rising).
4. Verse 1: `effects_on_marks` on "Music - All" with `section:"Verse 1"`, a white Pulse alternating the mini trees
   (`rotateTargets`); a second one on the bars stepping up the tiers. Then `passage_copy`
   `{section:"Verse 1", toSections:"Verse*", repeat:true}`.
5. Chorus 1: `motif_place` `slam` on "Music - All", `section:"Chorus 1"`, one target set per prop in `leftToRight`
   order: a chase across the yard on every beat. One `effects_batch` for meteors on the strips and a glow on the white
   tiers. Then `passage_copy` `{section:"Chorus 1", toSections:"Chorus*", repeat:true}`: the 19 s pattern fills the
   26 s and 41 s choruses.
6. Bridges: one `effects_batch` of twinkles on the centre tree.
7. `sequence_summary` with `previewId` → one problem: Large Bulbs dark through Chorus 1 (white slams on a green-only
   string). `effects_edit` `{section:"Chorus*", targetIds:[<Large Bulbs>], color:"#0aff00"}` fixes all three
   choruses; `effects_edit` `{section:"Bridge*", types:["Twinkle"], levelScale:0.6}` calms the bridges.
8. `sequence_summary` → `ok`. `sequence_render`, `render_analyze` → no dark stretch, no spazzing, changes on the beat.
   `sequence_save` with `label:"walk v2"`.

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
- Save explicitly (`sequence_save`); unsaved headless documents end with the session.

## Building notes

- **Pick effects by their look first.** `effect_catalog` (and `effects_library`) give each effect a `looks` text: what it
  does on a megatree, matrix, arch or line in plain words. Two surprises it spells out: plain Vertical `Wipe` moves
  *downward* (set ReverseDirection to rise), and `Chase` and `Spin` on a megatree group light only a one-string sliver
  at a time unless DepthOfEffect is 1, which steps whole strings around the trunk.
- **Match effects to props.** One-channel props get level effects (Pulse, Set Level, Twinkle across several of them);
  movement belongs on pixel props, or across props with `motif_place` target sets. Fixed-colour props show only their
  own colours. `sequence_summary` catches what slipped through.
- **Target by where lights sit**, not by guessing string numbers: `targetRegion` on `effect_add`, `effect_band`,
  `effects_batch` and `section_rebuild` ops takes `elementId` plus `angleDeg` (megatrees, 0 front, 90 viewer's right,
  `[315,45]` wraps through the front), `heightPct` (along the strings from the base), `xPct`/`yPct`, measured exactly as
  `display_geometry` reports them. Whole strings that match are one target each; for a height range on a whole tree,
  `effect_band` is one Wipe per string instead of one effect per light.
- **Motifs:** pick a built-in (`motif_list`) or save your own with `motif_save`; two `targetSets` alternate left/right,
  more walk across them in order. `palette`, `ramp` and `levelFromStrength` vary it per hit.
- **Soft zones:** `effect_band` with `featherPct` (or `featherLights`) instead of tuning a parked Wipe by hand; add
  `exclusive:true` and the band cuts whatever was lighting its rows for its time, so stacked zones never overlap.
- **Builds and hand edits:** to rebuild one section, use `section_rebuild` with the section's `range` and a `buildId`
  (e.g. `"chorus-2 v3"`) instead of deleting and re-adding effects: it replaces only that range, tags the new effects,
  and refuses when Braden has hand-edited the section since your build, listing the edits; read them with
  `build_edits`, then rebuild around them with `onHandEdits:"keep"`, or ask before using `"replace"`. Pass `buildId`
  to `effects_batch`, `passage_copy`, `effects_edit`, `motif_place`, `effects_on_marks` and `effect_step` too, so
  `build_edits` and `section_rebuild` see their effects as the build's. Run `build_edits` at the start of a session on an existing show.
- **Porting an old show:** start with `effects_port_from_sequence` `dryRun:true` and `autoZones:true` on the old `.tim`:
  it lists the zones (the old rig's channels grouped by name) to route. Take out the old rig's lag with `offsetMs`
  (e.g. -35), snap with `grid:"16th"` and a `snapToleranceMs` so hand-placed syncopation isn't forced onto the grid,
  read `moves.furthest`, then add `map` (zone → Studio targets) and run it for real with a `buildId`.
  `marks_import_from_sequence` takes the same options when you only want the old timing as marks.
- A step that takes several calls isn't atomic: if one fails, read the state (`sequence_summary`, `effects_find`)
  before retrying, or undo, so nothing is placed twice.

## Checking

- `sequence_summary` (structure and problems, seconds) before `render_analyze` (taste, from the rendered schedule).
- Before showing anyone a build, run `render_analyze` on the sequence (`scope` your main prop, `marks` your beat
  collection, `palette` your colours). It flags what viewers reject: dark stretches (a dark intro), spazzing (more than 6
  whole-prop changes a second), changes on no mark, and sections that lean blue/purple or leave the palette. Fix what it
  flags, starting from `jumpTo`, then run it again until `ok` is true; watch a `preview_video` of the flagged times only
  when the report isn't enough. It changes nothing.
