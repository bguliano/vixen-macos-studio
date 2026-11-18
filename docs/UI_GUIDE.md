# Vixen Studio interface guide

Studio is a redesign, not a replica. Windows Vixen's features are the starting inventory, but its layouts, wording and
workflows are not requirements. Keep a capability when a show needs it; change how it is reached when a better Mac way
exists. Vanilla's parity notes in `docs/parity/` remain useful as a checklist of what exists.

## Principles

1. **Apple Human Interface Guidelines first.** Current macOS design: Liquid Glass toolbars and sidebars,
   `NavigationSplitView`, `.inspector`, native `NSToolbar`/`.toolbar` (customisable), grouped `Form`s, SF Symbols,
   system colours and the accent colour. Follow the system's light or dark appearance; the timeline and preview canvases
   stay dark, as in Logic and Final Cut.
2. **One show window.** A sidebar holds Sequences, Display, Controllers, Previews and Schedule. Sequences open as
   document windows (tabbable). Setup tools are panes of the show window, not separate modal windows.
3. **One obvious way.** Each action has one canonical control in one predictable place, one word per concept, labelled
   controls, and every disabled or stale state explained in place. Shortcuts and gestures accelerate, never replace.
4. **Live edits with Undo** instead of OK/Cancel windows. Changes apply immediately and are undoable from the Edit menu.
5. **State is visible.** Rendering progress, live output, execution state, unsaved changes and agent activity are always
   shown, never discovered by surprise.
6. **Agents are first-class users (MCP parity).** Every user action is a command in the Rust session layer, called by
   both the SwiftUI interface and the built-in MCP server. Never implement behaviour only in Swift: if a button can do it,
   an agent can do it, and if an agent does it, the interface shows it live and Undo reverses it. Agent edits are
   labelled in the undo history ("Undo Add Effect (Agent)") and the toolbar shows when an agent is connected or working.
   When adding a UI feature, add or reuse its command and its MCP tool in the same change.
7. **Guide, don't gatekeep.** Empty states (`ContentUnavailableView`) say what to do next. Common confusions found in
   the Vixen community, such as effects that render nothing because of colour handling, get inline explanations and
   one-click fixes.

## Mac conventions

| Original | In Studio |
|---|---|
| In-window menu strip | The menu bar, with `.commands` |
| Ctrl shortcuts | ⌘ equivalents; single-key shortcuts unchanged |
| Options dialogs | The `Settings` scene (⌘,) |
| Modal dialog windows | Inspectors and sheets on the owning window; separate windows only for tools used alongside |
| MessageBoxForm | `.alert` / `.confirmationDialog`, with verbs on buttons ("Delete", not "Yes") |
| Open / Save dialogs | `.fileImporter` / `.fileExporter`, plus drag and drop |
| Draggable WinForms tool strips | A native customisable toolbar (View ▸ Customize Toolbar…) |
| Bottom tab rows of panels | Sidebar sections or an inspector with a segmented picker |
| Vixen's tool icons | SF Symbols; Vixen's effect icons stay |
| Tooltips | `.help("…")` |

Right-click menus use `.contextMenu`, Undo/Redo use the window's `UndoManager`, destructive actions use
`role: .destructive`.

## Modern SwiftUI, concretely

- Swift 6 language mode. Modules here default to `MainActor`; mark background work `nonisolated` or `@concurrent` and don't touch the UI from it.
- State in `@Observable` classes, passed with `@Environment` or `@Bindable`. No `ObservableObject`, no singletons beyond the app model.
- `NavigationSplitView`, `HSplitView`, `Inspector`, `Table`, `Form` (`.formStyle(.grouped)` in settings), `GroupBox`, `DisclosureGroup`, `LabeledContent`, `ContentUnavailableView`, `Grid` for aligned dialog fields.
- Multiple windows are scenes: `Window` / `WindowGroup(id:for:)` with `openWindow`. Documents go through `DocumentGroup`.
- Don't use AnyView or GeometryReader where a layout container does the job. No fixed pixel sizes except where the original's proportion matters. Windows must resize sensibly.

## Performance rules

The original's slowness is why the owner chose Rust. The UI must never bring it back.

- **Canvases and big lists are AppKit**, wrapped in `NSViewRepresentable`: the timeline, element trees (`NSOutlineView`, which is virtualized; 1,000+ rows is the norm and 50,000 must work), the preview. Never put thousands of rows in a SwiftUI `List`, and never use a SwiftUI `ForEach` per pixel or per effect.
- The preview is drawn by Rust on its own thread into a `CAMetalLayer`. Swift only forwards events to it.
- Never poll Rust faster than the display. Read playback position with `TimelineView(.animation)` or a display link, and move the play cursor as a separate `CALayer` so the grid doesn't redraw every frame.
- Fetch from Rust in bulk and cache in Swift; don't call across FFI per row per draw. Rendering, sorting and searching happen in Rust, off the main thread.
- A drag must never block on disk or re-render whole documents.

## Project structure

```
app/Package.swift
app/Sources/VixenKit         Shared: Theme, tree view, curve and gradient views, AppModel, WindowID, UI test harness
app/Sources/VixenApp         @main: puts the app together from whichever area modules a build has
app/Sources/Vixen<Area>      One module per window area: Admin, Dialogs, Setup, Preview, Timeline, EffectEditor, Marks, Sequencer, Scheduler, Tools
crates/vixen-ffi/src/core.rs The application object, VixenCore
crates/vixen-ffi/src/<area>.rs  The Rust side of each area's API (UniFFI), one Cargo feature per area
crates/vixen-session         What windows do, without the windows: sequences, undo, playback, timeline logic
```

The generated bindings aren't in `app/`. Each build generates its own, in its build folder.

An area's module exports a `Scene` named after it (`AdministrationScene`, `SetupScene`, `PreviewScene`, `SequencerScene`, `SchedulerScene`, `ToolsScene`) with an `init(model: AppModel)`. `VixenApp` includes it with `#if canImport`. **An area may use VixenKit and VixenBridge, and never another area**, so it can be built alone. The one exception: `VixenSequencer` assembles the window from `VixenTimeline`, `VixenEffectEditor` and `VixenMarks`. Each of those three also exports a small test scene (`TimelineTestScene`, `EffectEditorTestScene`, `MarksTestScene`) that the app uses when `VixenSequencer` isn't in the build, so it can be developed alone. `VixenSetup` may likewise use `VixenDialogs`, the property, filter and controller setup dialogs, whose test scene is `DialogsTestScene`. Areas open each other's windows by id, with `openWindow(id:)` and the constants in `WindowID`. Anything two areas need goes in VixenKit; add a new file rather than changing a shared one if you can.

The Rust side works the same way: `core.rs` and `sequence.rs` (the `SequencerSession` object: open, save, render status, transport, undo) are shared, and each `<area>.rs` adds its own `#[uniffi::export] impl` blocks or objects and depends on nothing but those.

Swift stays thin: no business logic in Swift. If behaviour exists in the C# non-UI code, it belongs in Rust and is exposed through
`vixen-ffi`. Validation, undo payloads, sorting, patching algorithms and file handling all live in Rust, so they can be tested without a window.

What's there to build on, in `crates/vixen-session`:

- `app::AppCore`: the open profile. `VixenCore.inner` is one. `session_environment()` gives what a sequence needs.
- `sequence::SequenceSession`: one open sequence, with stable `EffectId`s, background rendering (`render_all_dirty`, `apply_finished_renders`), the schedule, and save. Its edit methods record nothing; call `record_undo` with one `undo::UndoAction` for each user action.
- `playback::ShowPlayback` (`AppCore.playback`): play, pause, seek, position, execution open and close. One sequence plays at a time.
- `compare` and `tests/real_show.rs`: the whole pipeline against the owner's real exports. If that test starts failing, you've broken something real.

## Building and testing

```bash
source ./env.sh
scripts/build_app.sh debug        # Rust + bindings + Swift + bundle at build/Vixen.app
```

**Building alongside other workers:** give your build a name and list only the areas you need, so someone else's half-finished area can't break your build, and yours can't break theirs:

```bash
VIXEN_BUILD_ID=setup VIXEN_AREAS=setup,admin scripts/build_app.sh debug
```

That builds everything into `target/build-setup/` and the app is `target/build-setup/Vixen.app`. The sequencer window alone is the area `shell`; its parts `timeline`, `effecteditor` and `marks` can be added to the list, and `sequencer` means all four. The first build of a new id takes a few minutes; later ones take seconds. `admin` gives you the main window to start from. Keep your own Rust and Swift compiling between edits anyway: `crates/vixen-session` and `VixenKit` are shared.

### The UI test harness

The app can drive itself: no screen control, no focus stealing, no windows in the way. Set `VIXEN_UI_TEST` to a JSON script:

```bash
VIXEN_UI_TEST=/path/to/script.json VIXEN_UI_TEST_OUT=/path/to/out VIXEN_DATA=/path/to/profile \
  target/build-setup/Vixen.app/Contents/MacOS/Vixen
```

```json
{ "steps": [
  { "action": "window", "id": "display-setup" },
  { "action": "wait", "seconds": 0.5 },
  { "action": "click", "label": "Patch Elements" },
  { "action": "menu", "path": ["File", "Save"] },
  { "action": "snapshot", "name": "display-setup" },
  { "action": "dump", "name": "display-setup-controls" },
  { "action": "quit" }
] }
```

`menus` writes the menu bar as text, with each item's shortcut and whether it's off. `selectRow` with a `label` selects the table or list row showing that text, because a test window is never key and ignores a first click on a table. `closeWindow` closes the current window as its close button would. Alerts are reachable: `dump` lists their text and buttons, and `click` presses them. An alert's text is light on transparent, so its snapshot looks mostly blank. Under test the app writes what it would hand to another app, such as a browser address, to `notes.txt`.

`popup` (`x`, `y`, `label`, and `button: "right"` for a context menu, and `path` to walk submenus first) clicks a pop-up or menu button at those window coordinates and chooses by title (`name` can instead locate its currently displayed text, with optional `index`), and `editCell` (`x`, `y`) starts editing a table cell. A plain click can't do either here: SwiftUI draws its own pop-ups, and a test window never takes a first click on a table.

`snapshot` writes `<name>.png` of the key window and `dump` writes the window's accessibility tree as text, to `VIXEN_UI_TEST_OUT`. **Read those PNGs
with your Read tool and look at them.** "It compiled" doesn't mean it works: open every window you build, exercise its controls, and check the picture against
the reference. Results go to `VIXEN_UI_TEST_OUT/result.json`, with any step that failed.

Real data for testing is in `~/Developer/vixen-show-original/Vixen 3` (a read-only copy of the owner's real show). Never point the app directly at
it. A copy for tests is at `target/show-copy` (its `Media` is a link back to the original, so only read it). Copy that folder to your own under `target/` if your test saves
anything, and pass it as `VIXEN_DATA`.

Set `VIXEN_UI_TEST_TRACE=1` to log each step before and after it runs when diagnosing a stalled script. The in-process accessibility reader handles both plain and attributed text returned by SwiftUI; some offscreen icon controls are still absent from that tree, so use posted native pointer events and verify their resulting state when label lookup cannot reach them.

Tests must not make sound or send anything to hardware or the network. The harness sets `VIXEN_MUTE=1`, and output controllers stay stopped unless a test starts them.
