# Agent gap analysis: building a whole show over MCP (B73)

How the MCP served an agent building a complete song show, where the agent had to compute or loop on its own side, and
what was added so it doesn't have to. The workflow that came out of it is in [MCP-AGENT-GUIDE.md](MCP-AGENT-GUIDE.md).

## The walkthrough

A full show for *Wizards in Winter* (Trans-Siberian Orchestra, 3:08) on a copy of the show profile: twelve props in the
main preview (a centre tree of six relay tiers, large bulbs and a star, two mini trees, two pixel strips along the
eaves). Built twice, using only MCP calls through `studio/python/vixen_studio.py`, the second time with the B73 tools.
B74–B81 (motifs, feathered bands, `music_rhythm`, `render_analyze`, porting, section rebuilds, tree targeting, palettes)
were already in place for both runs.

| | First run (before B73) | Second run (B73 tools) |
|---|---|---|
| MCP calls | 532 (485 of them `effect_parameters`, to copy one chorus) | 23 |
| Client-side steps | 7 (below) | 0 |
| Chorus copied to the other choruses | failed: the read format of a gradient isn't the write format | `passage_copy`, one call, repeated to fill longer choruses |
| Silent mistakes | a placement run twice (930 duplicate effects); 55 s of verses dark | none: `sequence_summary` flagged the one dark prop, `effects_edit` fixed it |
| `render_analyze` at the end | 3 dark stretches (55 s), 5 spazz windows, too many changes off the beat | no dark stretch, no spazzing, changes on the beat |

## Where the agent had to work on its own side (first run)

1. **What is the display?** `display_elements` gives 1136 nodes; `display_geometry` gives 2307 lights. The agent paged
   through every light and grouped them by name to learn which props exist and where they are, and still couldn't tell
   that ten of the twelve props are a single relay channel drawn as up to 194 dots.
   → **`display_layout`**: every prop with the element to target, lights, channels, `control` ("one channel",
   "channels", "pixels"), the colours it can show, centre and box as 0–100 of the display, area, orders (left to
   right, inside out, bottom to top) and mirror pairs.
2. **Effects that can't show.** The verses used `pingpong` and `stab_run` (Wipes) on one-channel relays, and later a red
   Pulse on a white-only string: all dark, no warning. Found only because `render_analyze` reported 55 s of darkness,
   then `sequence_activity` per prop.
   → `display_layout` reports `channels` and `colors` up front; **`sequence_summary`** (with `previewId`) names each
   prop that has effects in a section but stays dark all through it, with the likely reasons.
3. **Section times.** Every section-scoped call needed the section's milliseconds, looked up from `music_rhythm` on the
   client (and the last section's end equals the sequence length, which `effects_batch` refused by a float rounding).
   → `section` in place of `range` on `effects_on_marks`, `motif_place` and `effect_step`; `section` filters on the new
   tools; an effect ending under 1 ms past the end is trimmed instead of refused.
4. **Spatial chase order.** Sorting props left to right for a chase across the yard, by hand from pixel boxes.
   → `display_layout` `order.leftToRight` etc., straight into `motif_place` `targetSets`.
5. **Copying a chorus.** `sequence_effects` has no parameters and no time filter, so the agent read each of 485 effects'
   parameters, shifted times itself, and the batch then failed: `effect_parameters` reports gradients as
   `{colors:[{color,position}]}`, `effects_batch` wants a flat colour list.
   → **`passage_copy`**: a range or section to other times or to every matching section (`"Chorus*"`), native copies
   with every parameter intact, `repeat` to fill longer sections, `targetMap` to retarget or mirror, `replace`, one
   undo step.
   Every write tool now also takes the gradient shape `effect_parameters` reads, so read values write back as-is.
6. **A failed multi-call step left duplicates.** A batch failed after its motif had been placed; the retry placed the
   motif again. Nothing reported the 930 duplicates.
   → `sequence_summary` flags duplicate effects (same type, target, times and parameters).
7. **Bulk edits.** Changing the colour of every effect on one prop across three choruses, or dimming the bridge
   twinkles, meant listing, filtering and an `effects_batch` of params ops built by hand.
   → **`effects_find`** (filter by range or section, type, target, build, layer) and **`effects_edit`** (params,
   colour, level scale, shift, stretch or delete for everything a filter picks, one undo step, dry run).

## Checked and already covered

Song structure (`music_rhythm` sections, `sequence_section_marks`), beat and bar grids (`sequence_music_marks`,
`music_rhythm` tempo map), effects on every beat with alternation across props (`effects_on_marks` `rotateTargets`,
`motif_place` target sets), palettes (`sequence_palette`), taste checks (`render_analyze`), contact sheets
(`preview_sheet`, and per section `preview_section_sheet`, B98), rebuilding one section (`section_rebuild`), "what is
on at time t" (`effects_find` with `range:[t, t+1]` and `match:"overlaps"`), intensity that follows the song's loudness (`effects_edit` `levelFromEnergy`
shapes each effect's level curve from the sequence's audio, B94), placers that tag their own effects for later
rebuilds (`motif_place`, `effects_on_marks` and `effect_step` take `buildId`, B96).

## Left for later (internal backlog)

- No warning at placement time when a movement effect or an impossible colour goes on a one-channel or fixed-colour prop
  (`sequence_summary` finds it afterwards).
- Lyric and phoneme timing for singing faces from lyrics text was not part of this walkthrough.
