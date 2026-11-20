# Studio UI roadmap (2026-11-20)

Evidence: an on-screen tour of every main surface on a copy of the real show (show window sections, sequencer with
"Christmas Eve Sarajevo", live preview, Scheduler, Set Up Previews), a code inventory, timing, and the friction the owner
hit while building the 896-pixel RGBW tree. Measured speed is good (a 692-effect sequence opens in 0.45 s; the preview
draws every changed frame at the engine's 100 Hz), so this plan is about layout and workflow, not raw speed.

Every item follows `docs/UI_GUIDE.md`: the behaviour lives in a Rust command with an MCP tool, the UI calls the same
command, edits are live with Undo, and each change is verified with harness runs plus Studio Capture screenshots.

## What the tour found

- **Two of four sidebar sections are launchers.** Previews shows a count and a "Set Up Previews" button that opens a
  vanilla OK/Cancel sheet (checkbox column with no label, an "Update" button for renaming, a redundant Type column).
  Schedule shows one "Open Scheduler" button that opens a separate vanilla window (typos in its copy, truncated show
  names, Close/Help footer).
- **Controllers have no home.** The guide promises a Controllers section; today they are the right column of Display.
- **71 OK/Cancel-style buttons remain** outside the redesigned areas: Marks 23, Scheduler 14, Tools 9, Admin 7,
  Dialogs 7, Preview 6, Setup 5. Each is a modal edit the guide says should be live with Undo.
- **About 200 fixed `.frame(width:)` sizes** in redesigned and vanilla views; windows don't adapt well.
- **Sequencer state is hidden.** No unsaved or render-progress indicator, no Save in the toolbar, an empty inspector
  that takes a third of the width when nothing is selected, a vanilla "Timeline - song.mp3" dock tab, tall rows, and beat
  marks drawn as full-height dashed lines that crowd the grid. The window subtitle shows the profile folder name.
- **The preview is a bare floating window** titled "(OpenGL)" with an FPS readout reading 0 when idle.
- **The tree build exposed the biggest gaps**:
  - E1.31 destination defaults to multicast over Wi-Fi with the IP hidden behind a "+".
  - Universes are added one row at a time, defaulting to 510 channels.
  - Templates have no RGBW option, and "Zig Zag Every N" is unexplained. (Fixed: Phase 1 items 2 and 3.)
  - Swapping a group or reversing one string means doing output arithmetic by hand.
  - A preview has to be built and linked separately from the element it shows.
  - Beat detection was unreachable (fixed in b17c7e2a6).
  - Agents can't set RGBW colour handling (`element_color` is rgb/single only). (Fixed: Phase 1 item 2.)

## Phase 1: hardware setup without arithmetic (do first; show season is now)

1. **Controller inspector rebuilt.** Protocol (E1.31 / DDP), unicast IP field up front, "channels per universe"
   (510 / 512) and start universe, with universes generated from the output count. A WLED preset ("Multiple RGBW" → 512)
   and a status line (IP reachable, last packet sent). MCP: extend `controller_configure`. *Done:* in Display ▸ Controllers'
   inspector (`StudioControllerInspector.swift`, `studio_controller_network.rs`); MCP `controller_configure` and
   `controller_network`. The universe table and multicast stay under Advanced….
2. **RGBW colour handling** in the element templates (Colour: RGB / RGBW / Single) and in `element_color`, with a
   one-line explanation of what each does to the channel count. *Done:* a Colour picker in the Single Item, Megatree and
   Pixel Grid sheets (`StudioTemplateColor.swift`, `studio_color.rs`) applied instead of the Color Handling sheet, a
   channel-count note under Color Handling's Color Order, MCP `element_color` mode `rgbw` and `element_template` `color`.
3. **Megatree and matrix templates explain themselves.** A live wiring diagram (start corner, zig-zag shown as arrows),
   "Reverse every other string" wording, "hidden pixels per string" (the tucked lights) that builds a separate group.
   *Done:* a Wiring section in the Megatree and Pixel Grid sheets (`StudioTemplateWiring.swift`, `studio_wiring.rs`):
   start corner (a megatree's named "Bottom of S1" … "Top of S32"), "Reverse every other string", "Hidden pixels per
   string" into "<name> Hidden", and a live diagram with sentences under it. Every light gets its wire position as its
   Order. MCP `element_template` `start`/`reverseEveryOtherString`/`hiddenPixels`, `patch_elements` `alsoIds` and wire
   order.
4. **Patching by intent, not numbers.** Context-menu actions on a string or group: Reverse, Swap with…, Move to output…,
   with the result shown as universe:channel ranges. MCP: `patch_reverse`, `patch_swap`. *Done:* a Patch submenu on one
   element or group in the Display pane (`StudioPatchMenu.swift`, `studio_patch.rs`): its ranges as universe:channel,
   Reverse, Swap With (elements using the same number of outputs, siblings first) and Move to Output (free runs that fit),
   each one undo step; MCP `patch_reverse`, `patch_swap`, `patch_move` and `patch_ranges`.
5. **"Show in a preview" from any prop.** Creating a megatree/matrix offers to add a matching, already-linked shape to a
   preview. *Done:* a Show in Preview picker in the Megatree, Pixel Grid and Arch sheets
   (`StudioTemplatePreview.swift`, `studio_props::add_linked`) adds the matching Mega Tree, Pixel Grid or Arch, linked
   light for light in the prop's order; MCP `element_template` `previewId` and `preview_prop_create` `elementId`.
6. **Commissioning tests** per controller and per group: moving pixel, R/G/B/W bars, at a chosen brightness (default 25%),
   gated by the live-output switch. Mirrors the build manual's Part J checks.
7. **Command palette (⌘K)** over every menu command and MCP-backed action, so nothing is unreachable again.

## Phase 2: finish the show window

1. **Previews section becomes a real pane.** Preview list with live thumbnails, enabled toggle, inline rename,
   duplicate/delete with Undo, and the designer opening in place. The Set Up Previews sheet and its OK/Cancel go away.
   Drop "(OpenGL)" from window titles; hide the FPS readout unless asked.
2. **Schedule section becomes a real pane.** Shows and schedules inline, next-run time, enable toggles, live edits with
   Undo. The Scheduler window is retired; copy fixed.
3. **Controllers section** in the sidebar, with run state, IP, universes and output health, sharing the Phase 1
   inspector.
4. **Sequences library**: real modified times, rendered/unsaved/missing-audio badges, "New Sequence from Audio…" by
   dropping an MP3, a context menu (Reveal, Duplicate, Export FSEQ).

## Phase 3: sequencer clarity

1. Title shows the song and an unsaved dot; the toolbar gains Save and a render-progress ring.
2. Inspector, when nothing is selected, shows the sequence: length, audio, marks collections, layers, render state.
3. Marks draw as coloured ticks in the ruler and waveform, with full-height lines only for the selected collection.
4. Compact / regular row height (View menu), remembered per sequence.
5. Replace the vanilla dock tab with the native window tab bar.
6. Effects library: favourites and recently used at the top.
7. Agent edits flash briefly on the timeline, matching the toolbar's agent activity.

## Phase 4: retire the remaining modals

Work through the 71 OK/Cancel flows in order of use: Marks (23), Scheduler (14, mostly gone after Phase 2), Tools (9),
Preview designer (6), Dialogs (7). Each becomes an inspector or a live sheet with Undo; confirmations use verbs on buttons.

## Phase 5: polish and guards

- Replace fixed widths with adaptive layout; check every window at its minimum size and in light mode.
- Accessibility labels and full keyboard navigation on custom controls (timeline, patch graph).
- Regression guards in the test suite: sequence open time, memory per agent edit (the e8a531dbc class of bug), preview
  frame pacing.

## Suggested order

Phase 1 (about 2 weeks of work, in this order: 1, 2, 4, 6, 3, 5, 7), then Phase 2 (about 2 weeks), then Phases 3 to 5 as
ongoing work. Each item ships on its own with tests and screenshots.
