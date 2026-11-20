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
   the output graph, so they work with Live Output on. Edit Layout opens the designer in its own window, and its
   write-back is one undo step, "Edit Preview Layout" (opening it in place is #58). System ▸ Setup Previews shows the
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
