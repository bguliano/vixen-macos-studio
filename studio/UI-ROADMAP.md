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
  (Fixed: Phase 2 item 3.)
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
   gated by the live-output switch. Mirrors the build manual's Part J checks. *Done:* a Test submenu on one element, group or
   controller in the Display pane (`StudioTestMenu.swift`, `studio_commission.rs`): Moving Pixel (wire order), Color Bars
   (R, G, B, W) and All On at a Brightness from 10 to 100%, with a banner and Stop Test while one runs; it needs Live
   Output on, so the trees stay browsable while editing is paused. MCP `output_test_pattern` patterns and
   `output_test_stop`.
7. **Command palette (⌘K)** over every menu command and MCP-backed action, so nothing is unreachable again. *Done:* View ▸
   Command Palette… (⌘K) in every window (`StudioCommandPalette.swift`, `studio_palette.rs`): reads the live menu bar of the
   front window each time it opens (hidden menus left out), ranks by title words, initials, menu names and keywords, shows
   each shortcut, why a disabled command is off, and the MCP tool that does the same; Return runs the command in its window.
   MCP `command_palette` searches the same list. Harness: `scripts/ui/studio-command-palette.json` with
   `scripts/check_studio_command_palette.py` proves every menu item in the show window and the sequencer is found by its
   own title.

## Phase 2: finish the show window

1. **Previews section becomes a real pane.** Preview list with live thumbnails, enabled toggle, inline rename,
   duplicate/delete with Undo, and the designer opening in place. The Set Up Previews sheet and its OK/Cancel go away.
   Drop "(OpenGL)" from window titles; hide the FPS readout unless asked.
   *Done:* the show window's Previews section (`StudioPreviewsPane.swift`, `studio_previews.rs`): a card per preview with
   a thumbnail of its layout (background and lights, redrawn on every change), an On switch (`DisabledDevices`: an on
   preview shows whenever Live Output is on), double-click rename, Duplicate, Delete and Add Preview, each one step on
   the shared undo history (Edit ▸ Undo and the pane's Undo button) and saved at once; they publish without rebuilding
   the output graph, so they work with Live Output on. Edit Layout swaps the pane to the designer
   (`StudioDesignerPane.swift`, registered through `StudioPreviewDesignerPane` in VixenKit) with a back button to the list:
   the designer attaches to the show window, so Edit ▸ Undo and the Edit actions reach it down the window's responder
   chain, its menus (File, View, Edit, Settings, Help) follow it while it shows, and its alerts, sheets and floating panels
   attach there; the show window's frame isn't saved as the preview's. Leaving it (the back button or another section) asks
   what closing its window asked (invalid links, OpenGL) and writes the layout back as one undo step, "Edit Preview
   Layout"; quitting writes it back without asking. System ▸ Setup Previews shows the
   section; the sheet is gone. Live windows leave "(OpenGL)" out of the title, and the frame rate shows only after
   Show Frame Rate in the window's menu. MCP `preview_create`, `preview_rename`, `preview_duplicate`, `preview_delete`,
   `preview_enable`; `preview_list` reports `enabled`. Harness: `scripts/ui/studio-previews.json` with
   `scripts/check_studio_previews.py`.
2. **Schedule section becomes a real pane.** Shows and schedules inline, next-run time, enable toggles, live edits with
   Undo. The Scheduler window is retired; copy fixed.
   *Done:* the show window's Schedule section (`StudioSchedulePane.swift`, `studio_schedule.rs`): the scheduler's switch
   (Scheduler ▸ Enabled, local only) with its status, every schedule with its show, days, times, dates, next run
   ("Tonight at 5:30 PM", Ended, No days ticked) and an On switch, opening in place to edit its show, first and last day,
   start and end time and days; every show with its steps by section (Before the show, Show, Alongside, After the show),
   double-click rename, Add ▸ sequence or pause, change, move and remove per step, Duplicate and Delete (a show's
   schedules go with it). Each edit is one step on the shared undo history and saved at once, published without
   rebuilding the output graph, and handed to the running scheduler, so a change applies at once and a playing show
   keeps playing unless its schedule changed. Scheduler ▸ Shows and Schedules show the section; both windows are gone.
   Launch steps (running a program) are listed but not created (#59). MCP `show_create`, `show_rename`,
   `show_duplicate`, `show_delete`, `show_item_add`, `show_item_update`, `show_item_move`, `show_item_delete`,
   `schedule_create`, `schedule_update`, `schedule_enable`, `schedule_duplicate`, `schedule_delete`; `show_list` and
   `schedule_list` report what the pane shows. Harness: `scripts/ui/studio-schedule.json` with
   `scripts/check_studio_schedule.py`.
3. **Controllers section** in the sidebar, with run state, IP, universes and output health, sharing the Phase 1
   inspector.
   *Done:* the show window's Controllers section (`StudioControllersSection.swift`, `studio_controller_network.rs`
   `overview`): a list of every controller with its run state, address, universes, patched outputs and output health
   (the reason in words: no address, nothing patched, universe table errors, send errors, stalled packets), refreshed
   every second, beside the controller inspector and Display's controller actions (Start, Stop, Advanced…/Configure…,
   Outputs…, Rename…, Find Elements, Order, Delete, Add). A context menu offers the same plus Test and Unpatch; double-
   click opens the settings and Delete deletes. It is the Display pane's live working copy shown another way: its
   selection is Display's controller tree selection, every edit is one step on the shared undo history, and moving
   between Display and Controllers keeps the working copy open. Find Elements moves to Display, where the elements
   are selected. MCP `controller_overview`. Harness: `scripts/ui/studio-controllers.json` with
   `scripts/check_studio_controllers.py`.
4. **Sequences library**: real modified times, rendered/unsaved/missing-audio badges, "New Sequence from Audio…" by
   dropping an MP3, a context menu (Reveal, Duplicate, Export FSEQ).
   *Done:* the Sequences section's rows carry badges (`StudioSequenceLibrary.swift`, `studio_library.rs`): Unsaved
   and Rendering n% / Rendered while a sequence is open, Audio Missing when Media lacks its song, and FSEQ when the
   Export folder's FSEQ is newer than the saved file; open sequences refresh every 2 s. Dropping a song (or the New
   Sequence menu's New Sequence from Audio…) copies it into Media, saves Sequence/<song>.tim as long as the song and
   opens it. The context menu adds Duplicate ("<name> copy") and Export FSEQ (to the Export folder, with Show in
   Finder in the status line) beside Open and Show in Finder. New Sequence from Audio and Duplicate are on the
   window's Undo, which removes the file only while it is unchanged and closed. MCP `library_list`,
   `library_new_from_audio`, `library_duplicate`, `library_discard`, `library_export_fseq`. Harness:
   `scripts/ui/studio-library.json` with `scripts/check_studio_library.py`.

## Phase 3: sequencer clarity

1. Title shows the song and an unsaved dot; the toolbar gains Save and a render-progress ring.
   *Done:* the sequencer window's title is the sequence and its subtitle the song (Windows paths stripped; "(audio
   missing)" when Media lacks it; "No audio"), with the close button's edited dot until saved
   (`StudioWindowChromeSync.swift`, `studio_window_chrome.rs`), refreshed on every poll so renders and agent edits
   show without a UI change. Save is in the default toolbar, and the clock carries a render ring (rendered of all
   effects, as the Sequences badge counts) that becomes a checkmark when every effect is rendered, its tooltip the
   counts. MCP `sequence_status` reports the same title, subtitle, song, songMissing, rendering and renderingPercent.
   Harness: `scripts/ui/studio-window-chrome.json` (hook `sequencer.chrome`).
2. Inspector, when nothing is selected, shows the sequence: length, audio, marks collections, layers, render state.
   *Done:* with no effect selected, the Effect Editor tab shows the sequence instead of an empty placeholder
   (`StudioSequenceInspector.swift`, `studio_sequence_inspector.rs`): a Length field (m:ss.fff, m:ss or seconds), the
   song with its length, a missing-audio warning, Fit Length to Song and Change Audio…, every mark collection with its
   mark count and a switch that shows or hides its lines and bar, the layers with their effect counts (renamed in place,
   Add Layer), and the render progress. Length, mark visibility and layer edits are each one step on the window's Undo
   (mark visibility through the new `StudioMarkVisibility` undo action); changing the song is still the Audio menu's,
   without Undo, as in vanilla. MCP `sequence_overview` reads the same, `marks_visibility` toggles collections, and
   `sequence_length` also takes the field's text as `length`. Harness: `scripts/ui/studio-sequence-inspector.json`
   (hook `sequencer.inspector`).
3. Marks draw as coloured ticks in the ruler and waveform, with full-height lines only for the selected collection.
   *Done:* the ruler draws each mark as a solid tick in its collection's colour, rising from the bottom edge (wider when
   selected), and the waveform as short ticks at its top and bottom edges (`StudioMarkTicks.swift`, `studio_mark_ticks.rs`).
   Lines cross the grid only for the collections whose marks are selected (click a mark in the ruler or the marks bar;
   clearing the selection clears them), and for every collection with lines while an effect drag snaps, as an overlay
   that leaves the tiles alone, so snapping still shows what it can snap to. The collection's own colour, bold and dash
   still style those lines. Which lines show is the window's view, not the document, so there is no command or Undo for
   it; mark selection, visibility (`marks_visibility`) and snapping are unchanged. Harness:
   `scripts/ui/studio-quiet-marks.json`.
4. Compact / regular row height (View menu), remembered per sequence.
   *Done:* View ▸ Row Height picks Compact (20 pt rows) or Regular (32 pt), so a compact grid shows 1.6 times as many
   rows (34 against 21 in a 1400×900 window on Christmas Can-Can). Every row scales by the same factor, so hand-sized
   rows keep their proportions (`studio_row_height.rs`, `StudioRowHeight.swift`). The choice is the sequence's own
   `DefaultRowHeight` and row settings, which the file already carries, so it is remembered per sequence (saved
   silently at close like zoom) and vanilla and Windows open it as saved. The menu runs `sequence_row_height` through
   the window's document, as an agent does, and an open window follows the document at once. Like zoom it is a view
   setting, not on Undo: choosing the other height reverses it. Harness: `scripts/ui/studio-row-height.json`
   (remembered across close and reopen) and `scripts/ui/studio-row-height-interact.json` (drag, Undo, row selection
   and switching back in compact rows; hook `sequencer.rows`).
5. Replace the vanilla dock tab with the native window tab bar.
   *Done:* the "Timeline - song.mp3" tab strip above the grid is gone, so the toolbar sits directly on the ruler, and
   sequencer windows tab natively (`StudioSequencerTabs.swift`): a sequence opened while another sequencer window is
   open joins that window as a new, selected tab titled with the sequence (the window title Phase 3 item 1 sets), and only
   sequencer windows tab together. Dragging a tab out, Window ▸ Merge All Windows, Move Tab to New Window and Show Tab
   Bar are the system's own; each tab keeps its own document, Undo, inspector and close prompt. Tabs arrange windows,
   not the document, so there is no command or Undo for them (agents still open sequences with `sequence_open`).
   Harness: `scripts/ui/studio-window-tabs.json` with `scripts/check_studio_window_tabs.py` (hook `sequencer.tabs`).
6. Effects library: favourites and recently used at the top.
   *Done:* Favorites and Recently Used sit above the categories (`StudioEffectsLibrary.swift`,
   `studio_effects_library.rs`). Each effect's row has a star: pressing it pins the effect to Favorites (in the order
   pinned) and pressing it again unpins it; an empty Favorites says to click a star. Recently Used lists the five newest
   effects placed in the sequencer window (a drop on a row, Add Effect or a drag drawn in Draw mode), leaving out
   favorites, with Clear to empty it; agents' effects don't count, so a script can't push out what the user reached for.
   Both are hidden while searching. They belong to the user, not the show: kept in `Studio Effects Library.json` in the
   user's Vixen settings folder, so they survive relaunch and follow the user from show to show. The star and Clear run
   `effects_library`, as an agent does, and the window follows changes from agents or placements on its poll. Like row
   height they are the user's view rather than an edit to the sequence, so they aren't on Undo: the star, pressed again,
   reverses a pin. Harness: `scripts/ui/studio-effects-library.json` then
   `scripts/ui/studio-effects-library-relaunch.json` with the same `VIXEN_UI_TEST_OUT` (hook `sequencer.effectsLibrary`).
7. Agent edits flash briefly on the timeline, matching the toolbar's agent activity.
   *Done:* effects an MCP agent adds or changes in an open sequence light up once on the grid, outlined and filled in
   purple, and fade over about a second and a half (`StudioAgentFlash.swift`, `studio_agent_flash.rs`). Every agent edit
   notes the effects it added, moved, resized, re-layered or re-parameterised where it goes on the Undo stack, so no
   command can forget it; the window asks for what's new on its poll, once its grid shows the edit, and flashes those
   tiles in one layer. The user's own edits, Undo and Redo don't flash, and opening a sequence an agent edited earlier
   flashes nothing. Effects in collapsed rows or out of view count but have no tile to light. Like the toolbar's activity
   it is the window's view of the document, so there is no command or Undo for it; agents see their edits in
   `sequence_status` and `sequence_effects` as before. Harness: `scripts/ui/studio-agent-flash.json`, driven by
   `scripts/check_studio_agent_flash.py`, which plays the agent through the app's MCP helper and captures the window
   while it flashes (hook `sequencer.agentFlash`).

## Phase 4: retire the remaining modals

Work through the 71 OK/Cancel flows in order of use: Marks (23), Scheduler (14, mostly gone after Phase 2), Tools (9),
Preview designer (6), Dialogs (7). Each becomes an inspector or a live sheet with Undo; confirmations use verbs on buttons.

- **Marks (R40).** *Done for the Mark Collections panel:* every edit (New, Delete, rename, the checkboxes and header toggles,
  Color, Weight, Type, Linked To, Offset Time, Import) and Beats and Bars detection is one step on the sequence's Undo
  (`studio_marks_panel.rs`), which restores the whole collection list and the effects that read a deleted collection.
  Offset Time is a live popover on the row (its changes are one Undo step); Import is a menu of file types, so the Import
  Type dialog is gone; a single-colour Pangolin import comes in white and is recoloured from the row; the selection and
  export sheets confirm with verbs (Import, Export…). MCP: `marks_collection_edit`, `marks_collection_delete`,
  `marks_import`. Harness: `scripts/check_studio_marks_live.py` with `scripts/ui/studio-marks-live.json`. *Remaining:* the
  LipSync library editors in VixenMarks (Edit Image Maps and its map editor and Image Mapper, the Lyric Converter's
  unknown-word mapping, Face Mapping node selection), which edit the profile's LipSync library rather than the sequence and
  need the show window's shared history for Undo (backlog).

- **Preview designer (R41).** *Done:* the designer's sheets no longer commit through OK/Cancel. Linked Elements changes the
  shape as it's used (drop, double-click, Copy, Reverse, Set) and closes with Done; everything done while it's open is one
  step on the designer's Edit ▸ Undo ("Link Elements"). A moving head's Link Element links the chosen row at once ("Link
  Element", with Unlink and Done). Location Offset applies each field as it's committed ("Location Offset", Done). Resize
  Background confirms with Resize and is one Undo step that puts the old image and size back; Single Bulb Configuration
  says Draw Bulbs, Create a Template says Save Template, and the prop-naming prompt says Continue (`studio_design_undo.rs`
  in vixen-preview, `studio_preview_designer.rs` in vixen-ffi). MCP: `preview_location_offset` (one step on the shared
  history); linking and properties already had `preview_prop_link` and `preview_prop_set`. Harness:
  `scripts/check_studio_designer_live.py` with `scripts/ui/studio-designer-live.json`. *Remaining:* Tools (the prop
  editor's OK/Cancel sheets and name prompts) and Dialogs (the property, filter and controller setup sheets), each its own
  backlog item.

## Phase 5: polish and guards

- **Adaptive layout (R42).** *Done:* every window is checked at its minimum size in light and dark by
  `scripts/ui/studio-adaptive-layout.json` (`scripts/check_studio_adaptive_layout.py`) with the `studio.layout` harness hook,
  which also flags controls past a window's edge, overlapping siblings and truncated labels. Fixed: the sequencer's minimum
  was 640 points with side panes 200 and 340 wide, so the timeline went under the inspector; its panes now have minimums
  (180, 320, 260) that set the window's, a window resize takes from the timeline and squeezes a side pane only down to its
  minimum, and only a dragged divider is remembered (a resize used to save the squeezed widths for good)
  (`StudioDockLayout.swift`). The Controllers table's columns fit the show window's minimum and grow when it's wider; the
  Display pane's patch counts wrap instead of truncating; About opens at the original's 900 × 800 but shrinks to 640 × 480.
  The remaining fixed `.frame(width:)` sizes (about 300, mostly icons, swatches, sheets and form label columns) were left
  where the windows holding them pass the audit; sheets and the preview designer aren't in the audit yet. *Remaining:* the timeline's row-name column (inside the timeline) still shrinks
  in proportion with the window and saves that width, shared with vanilla (backlog).
- **Accessibility on the custom canvases (R43).** *Done:* the timeline grid, the Display pane's graphical patching and
  the preview designer are VoiceOver layout areas holding one labelled item per visible effect, shape or prop, with the
  words built in Rust (`studio_access.rs` in vixen-session and vixen-preview): "Set Level on White Bottom, 0:00.476 to
  0:01.497", "White Bottom, element, to Output 1", "Poly Line, 161 lights, linked to White Bottom". A press selects the
  item as a click does; on the graph and designer VO-⇧-M opens its menu. Keys are the same on all three: Tab and ⇧Tab
  select the next and previous item inside the canvas, ⌃Tab and ⌃⇧Tab move on, and a focus ring marks the canvas with
  the keyboard. The timeline adds ← and → through the active row's effects (⇧ extends; the first key gives a grid with
  no active row one), ⌥→ and ⌥← to open and close the row, and reads the row ↑ and ↓ move to; the graph's Return opens
  the selected shape's menu, whose Connect To lists what a drag from its output port could reach (Expand/Collapse and
  Configure Filter… too); the designer's arrows nudge the Tab-selected prop as before. `sequence_effects` gives agents
  the same `label`. Verified by `scripts/check_studio_access.py` (`scripts/ui/studio-access.json`, hook
  `studio.access`). The designer's arrow nudge isn't on Undo yet, as upstream (backlog).
- **Regression guards (R44).** *Done:* three checks in the test suite, each on `target/show-copy` (skipped without it).
  `crates/studio-mcp/tests/regression_guards.rs` drives the real MCP server: opening and rendering the sample show's
  busiest sequence (Wizards in Winter, 1,440 effects, about 50 ms) must take under 1 s, and 150 back-to-back
  `effect_add` calls on a 200-light megatree must add under 1 MB of resident memory each (about 180 KB healthy) and
  schedule every effect exactly once. `crates/vixen-session/tests/studio_preview_pacing.rs` plays two-second Pulse ramps
  on the show's display into a headless live preview of its largest preview (2,309 lights) and checks the engine keeps
  its 10 ms update interval and the drawn frames keep up (gap p50 under 12.5 ms, p95 under 20, none over 50; healthy is
  10.0 / 13.5 / 19). Each failed on a deliberate regression: e8a531dbc's dedupe turned off (13.6 MB per edit), 1 ms
  added per applied render (2.2 s open), 15 ms added per drawn frame (203 frames in 4 s instead of 400).

## Suggested order

Phase 1 (about 2 weeks of work, in this order: 1, 2, 4, 6, 3, 5, 7), then Phase 2 (about 2 weeks), then Phases 3 to 5 as
ongoing work. Each item ships on its own with tests and screenshots.
