# Where this port deliberately differs from Vixen

Each entry is a place the port does something other than what upstream's C# does, on purpose. The owner can veto any of them. Where the C#'s
behaviour is a quirk that the port **keeps**, it isn't listed here: those are marked "quirk" or "deliberate" in the code and the parity tables.

Anyone adding a difference adds a line here, with the reason and how to reverse it.

## What gets rendered

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| Whirlpool, Pause At End (`crates/fx-pixel-complex/src/whirlpool/mod.rs`) | Assigned after setup reads it, so a freshly loaded effect ignores it, and an edit shows only on the render after next | Always applied | The same file renders two ways in upstream depending on what happened in the session. Applying it gives what upstream's second render gives, checked against 1,172 golden cases. Your show has no Whirlpool effects | Assign `pause_at_end` after `setup_render` and drop `SettledOnSecondRender` from `tools/golden` |
| Glediator dirty rerender (`crates/fx-pixel-graphics/src/glediator.rs`) | Keeps `_speed` from the preceding render, changing playback phase even with identical inputs | Resets phase to the fresh-instance value for every render | Deterministic output/cache behavior; original-C# probes change186/330 element-time samples in Iterations/Speed, while fresh/reloaded output matches277 cases. The deterministic choice is retained | Retain per-instance Glediator accumulator state outside pure render functions |
| Chase (VIX-2276) | The build that exported your files had a bug that upstream has since fixed | Follows current upstream | Upstream's own fix. It accounts for 42 of the 48 values in your FSEQ files that differ outside the random effects, all in "Dance of the Sugar Plum Fairy" | Not recommended |
| Library links (`crates/vixen-session/src/library_links.rs`) | After a library edit, only effects whose `IsDirty` checks the link render again at once. The others catch up on their next change | All linked effects render again | Same end result, without the delay | Not recommended |

## Fixture previews and editors

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| Bottom-mounted fixture tilt | OpenGL preview adds 180°; WPF designer/editor applies tilt directly | Adds 180° consistently in every port fixture view | Owner selected the OpenGL convention on November 11, 2026 | Pass zero `tilt_offset_deg` for editor thumbnails |
| Fixture rendering | OpenGL and WPF use separate geometry and lighting engines; both beam halves are accidentally drawn under both OpenGL color shaders | Shared Rust volume mesh and GPU lighting; each half drawn once, editor view uses a clipped local camera and its original focus formula | Specialized spec F138–144 explicitly requests one renderer and removes duplicate draws; stable cached geometry avoids per-frame reconstruction when values are unchanged | Restore duplicate beam draws and a separate WPF-style lighting pipeline |

| Fixture profile validation | Incomplete assignment mutates the channel before reverting; unchecked End Value parsing can crash; new Pan/Tilt rows can lack limits | Assignment is atomic; nonblank DMX end fields are validated; new rotation limits are 0/360 | Explicit specialized-editor audit corrections; prevents invalid saved profiles | Restore the original setter/parse ordering |
| Function panel switching | Uncommitted detail edits can be lost and hidden panels can block OK | Valid details persist before type/tag changes; only the current type's data is validated | Explicit specialized-editor audit correction | Recreate hidden panel models on selection |
| Function timeline colors | Random walk across KnownColors can loop when exhausted | Fixed palette, selecting the first unused color and bounded fallback | Spec allows a documented fixed palette | Port the full KnownColors palette and bounded random walk |
| Fixture wizard revisits | Recreated page view models reset answers | Defaults recompute when dependencies change; ordinary revisits keep answers | Explicit specialized-editor audit correction | Reset page values at every entry |
| New fixture profile name collision | Silently overwrites library entry | Native confirmation before replacement | Explicit specialized-editor audit correction | Remove the new-profile confirmation gate |

## Files

| Where | Upstream | This port | Why |
|---|---|---|---|
| FSEQ compression blocks (`crates/vixen-export/src/fseq.rs`) | Flushed but unterminated zstd frames | Complete frames | Streaming decoders read both, and only complete frames work with one-shot decoders. Frames decode identically on all 14 of your files. The reader takes both |
| FSEQ with under 10 frames | `frames - 10` wraps around, declaring 255 blocks in a file that can't be read back | One block | Upstream's result is unusable |
| CSV export with no channels | Fails with an index error | Writes empty rows | |
| Sequence migration from version 0 | Copies reachable audio from its old path into Media during XML migration, and shows message boxes | Low-level XML decoding is read-only; opening a session or importing a package restores reachable media before saving migrated names. Existing Media files win; messages become UI notes | The owning operation knows the destination profile. Atomic no-overwrite publication avoids partial files while keeping the original sequence/source unchanged; unreachable Windows paths remain missing-media entries |
| Video effect frame-cache layout (`crates/vixen-video`) | ffmpeg caches scaled/cropped/rotated images in a shared temporary folder | The native decoder streams scaled frames to versioned, profile-isolated temporary cache folders; crop/rotation remains in the effect renderer. Both cache Options now act on actual files, and retained caches reopen from disk | Native decoding/layout differ; lazy frame access bounds memory, and active leases prevent one window deleting another's frames. See `docs/review/video-cache.md` |
| Video with stale loaded metadata | A loaded effect with stored duration 0 and ordinary quality can remain dark until its file setter refreshes metadata | The renderer can use actual media information while retaining loaded metadata/quality; selecting the file refreshes stored duration/size and resets quality to 50 | Existing macOS loaded-file recovery is preserved. Normal file selection and legacy quality 0 now follow the original refresh lifecycle. See `docs/review/video-metadata.md` |

## Export (`docs/spec/shows-scheduler-export.md`, parity in `docs/parity/shows-scheduler-export.md`)

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| Every export run (`crates/vixen-export/src/run.rs`, F12) | Truncates the target file the moment a run starts; a cancel or a failure leaves a broken file whose header declares more blocks than its table holds | Writes `<file>.part` in the same folder and renames it over the target only when the run succeeded. A cancel or failure deletes the part file and leaves any earlier export as it was | Your `Export` folder goes to the Pi as it is, so a broken file there takes a song down. A part file only exists while an export runs | Write straight to `out_file_name` in `run_export` |
| Export progress (`run_export`) | Reports every interval, 40 messages a second of show | Reports when the whole percent changes, at most 20 times a second, always including 0 and 99 | Upstream's rate floods the window for nothing | Report every interval |
| Export Sequence dialog, first run (F3) | With nothing saved, `CSV File` | `Falcon Player Sequence 2.6+`, upstream's own `DefaultFormatType()` | A first export on a new Mac would otherwise be a CSV. Your saved choice lives in `Settings.xml` on the Windows machine | `DEFAULT_FORMAT_INDEX` = 0 in `crates/vixen-session/src/export_dialog.rs` |
| Export Sequence dialog, Enable Compression (F8) | Enabled by the format at load, then also enabled by choosing FPP 2.x and disabled by FPP 1.x whatever the format | Enabled by the format alone: only `Falcon Player Sequence 2.6+` compresses | The radio rule is an accident; a universe file has nothing to do with compression. The value was only ever applied to a format that compresses, so files are unchanged | `compress_enabled` |
| Export Sequence dialog, the "controllers do not support universes" warning (F9) | After every export to a format named Falcon, whether or not a universe file is being written | Only when Generate Controller File is ticked | For your show, whose controllers have no universes, upstream warned on every export about a file it wasn't writing. Question Q2 below | `falcon_warning_needed` |
| Export Sequence dialog, a failed export (F12) | The exception escapes to the crash handler | An alert, `Export Failed`, with the system's message, and the dialog goes back to idle | | |
| Export Sequence dialog, out-of-range saved settings (F3, F4) | A saved index outside the list throws when the dialog opens | Falls back to the default | | |
| Export Sequence dialog, Timing (F4) | No range check: 0 divides by zero on a worker thread, and over 255 is truncated to one byte, so 300 ms declares 44 ms | Anything outside 1 to 255 gets upstream's `Invalid timing interval` alert | | |
| Export Wizard, universe file (F33, `crates/vixen-session/src/export_wizard/run.rs`) | With File Path, writes `config/co-universes.json` after every FPP 2.6+ export, with an empty list when Create Universe File is unticked, replacing the file a player already had | Writes it only when Create Universe File is ticked | An unticked box silently wiping the player's outputs is a trap | Drop the `create_universe_file` test in `wants_universes` |
| Export Wizard, last page (F36) | `Export Complete` even after a failure or a cancel | `Export Complete` for a success, otherwise the run's final text | | |
| Export Wizard, timing box (F26) | Reading whether Next is allowed also stores the typed timing in the profile | The timing is stored when typed; the Next test changes nothing | Same result, no hidden side effect | |
| Export Wizard, audio copy (F32) | Copying a sequence's audio onto itself (the audio folder is where the audio already is, Rename off) throws to the crash handler | Skipped | | |
| Export Wizard, folders from a Windows profile (F26, F32) | n/a | A stored folder that isn't an absolute path on this Mac, such as `C:\\...`, is never created; the stage asks for a folder instead | Your stored profile carries Windows paths | |
| Export Wizard, the Falcon note (F26) | Tells you to pick `\\\\hostname\\fpp` | Tells you to mount the player's media share and pick that folder | UNC paths have no Mac form | |
| Export Wizard, rendering at the profile's timing (F31) | Sets the global `DefaultUpdateInterval` for the length of the run and restores it | The timing goes to this run's own sessions; nothing global changes | Same frames, and a sequence open in the editor isn't disturbed | |
| FPP client, upload warnings (F76) | A non-OK `Status` in an upload's answer, or an answer that isn't JSON, is written to the log and the upload counts as done | The same, but the text comes back in `UploadOutcome.warning` for the caller to log | The crate has no logger of its own | |

## Sequence packages and element maps

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| Package output browsing (F83) | Starts in the saved folder only when it is missing | Starts in the existing saved folder; normalizes Windows folder paths and uses `show.vpkg` without a profile name | Native usable default | `ExportSettings::enter_output`, `PackageWizardModel.choose` |
| Package writing and finish (F84, F85) | Deletes the old archive before writing; partial files remain on cancel; declining overwrite still says complete | Writes a private `.part-<uuid>` sibling, checks cancellation between 64 KiB chunks, replaces only on success; finish reports actual outcome and errors | Keeps existing packages usable after cancellation or failure | `sequence_package::export`, `PackageWizard::finish` |
| Repeated media references (F84) | Stores duplicate ZIP entries with the same name and bytes | Stores each matching file once per sequence | The ZIP writer rejects duplicate names; extraction produces the same files, and upstream already skips existing media | Use a ZIP writer accepting repeated names and remove discovery deduplication |
| Package import (F88–F90) | Leaves temporary trees; retains old package rows; suffix match can import `BigFoo` as `Foo`; extracts before loading and broadly deletes migration backups | Tree stays in memory; new package replaces rows; entry filename matches exactly; load/remap happens privately before atomic replacement, so no migration backup is created | Avoids stale selections, wrong sequences, lost existing files and unrelated backup deletion | `PackageWizard::set_input`, `sequence_package::inspect` and `import` |
| Package summary/error text (F84, F90) | Calls import summary “Export”; export errors say “Error Importing Package” | Uses Import/Export for the operation actually running | Clarifies errors and headings | `PackageWizardView`, `PackageWizardModel.tell` |
| Element map editing (F92) | New/Open silently discard unsaved edits; clearing and loading a tree do not mark modified; save errors escape | Confirms replacement; clear/tree edits set modified; write failures keep editor dirty and show a saving error | Prevents losing edits | `ElementMapperModel`, `ElementMapState` |

## Shows and Scheduler (`docs/spec/shows-scheduler-export.md`, parity in `docs/parity/shows-scheduler-export.md`)

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| Renaming a show in the list (F40) | The new name is only saved if something else saves later | Saved at once | A rename could be lost on quit | `show_rename` in `crates/vixen-ffi/src/scheduler.rs` |
| Deleting an item in the show editor (F45) | Uses the stored order number as a list index, which throws when a stored tab has a gap | Uses the row's position | | |
| A Pause item stored as 0 seconds (F48, F63) | The editor throws when the action is switched to Pause, and a show with one never starts | The editor shows 1 and stores it only if you change it; at run time 0 completes at once | Every item in your store has 0 | |
| An item whose stored action isn't Launch, Sequence or Pause | The whole show list fails to load | The text is kept, no editor is shown, and it runs as nothing | | |
| Program Output Window (F48a) | Closing by the title bar leaves the program running; Close after a failed start throws | Closing either way stops it; a failed start just closes | | |
| Cancel in the show editor (F48b) | `YES`/`NO` | `Discard Changes`/`Keep Editing` | Mac alerts name the action | `ShowEditorModel.cancel` |
| Deleting a running schedule (F51) | It plays on to its end time and the Stop buttons can't reach it | Stopped as it's deleted | | `SchedulerSession.delete` |
| Edit the Associated Show (F52) | Edits the live show, so Cancel doesn't undo | Edits a copy, as the Shows window does | | |
| Schedule a Show, OK (F53) | Writes the fields before checking that a show is chosen, so Cancel after the alert keeps them; compares the dates with their time parts | Checks first; compares dates only, which gives the same answers | | `validate_schedule_edit` |
| The Schedule column (F49) and the log's time stamps (F58) | The Windows region's short date and time | Always the en-US forms, `12/25/2018 @ 7:07 PM` | One form on every Mac and in the tests | `crates/vixen-session/src/scheduler/mod.rs` |
| Scheduler Status window (F55) | A form with no control box | A panel with no close button, shown while the scheduler is enabled | | |
| The status log (F58) | Means to keep 250 lines but never removes any | Keeps the newest 250 | | `Scheduler` in `scheduler/run.rs` |
| Which show a schedule runs (F100) | The show object found the first time, until the app restarts, so an edited, re-pointed or deleted show keeps running as it was | Looked up when each run starts | | |
| Stop Gracefully before the first sequential item (F101) | The schedule sticks in `Shutdown` until the scheduler is turned off and on | Goes straight to the Shutdown items | | |
| Stop Show Immediately (F63) | Also kills a program that was started without Wait for Exit and already counted as finished | Leaves it alone | A finished action isn't running | |
| A Launch item's output (F63) | Read only at exit, so a program that writes a lot blocks forever | Read as it arrives | | |
| A show with nothing playable (F61) | Loops as fast as it can | Waits a second between passes | A show of unplayable sequences would otherwise spin a core | |

## Editing

| Where | Upstream | This port | Why |
|---|---|---|---|
| Undo (`crates/vixen-session/src/undo.rs`) | A new action leaves the redo list as it was, so Redo can act on effects that no longer exist | A new action clears redo, as every Mac app does | Upstream's can act on effects that have gone |
| Undoing a delete | Loses the effect's layer | Returns it to its layer | |
| Mark Collections panel, Tail Lines header (`crates/vixen-session/src/marks/mod.rs`, `TAIL_HEADER_FIX`) | Its first value starts inverted, so with no tail lines anywhere the first click does nothing visible | Worked out like the other headers: the first click turns them on | Spec F8's proposed fix. Set the constant to `false` to reverse |
| Mark Collections panel, Offset Time | Changes the marks without telling anything: the sequence isn't marked modified and the grid keeps the old lines until something else redraws | Marked modified, and the timeline and dependent effects follow | Spec F18's proposed fix; otherwise the change could be lost without a save prompt |
| Mark Collections panel, Linked To | A link change doesn't mark the sequence modified | It does | Spec F24's proposed fix |
| Mark Collections panel, Weight and Type submenus | Choosing the checked item unchecks it until the panel is rebuilt | A native picker: the current value stays checked | Spec F22, F23. No data difference |
| Mark Collections panel, two sequencer windows | Rows check names and links against the first window's sequence | Each panel uses its own sequence | Spec F115 |
| Mark export to `.v3m` | A failure is only logged | An alert says the export failed | Spec F38's proposed fix |
| Beats and Bars dialog (`app/Sources/VixenMarks/BeatsAndBarsSheet.swift`) | Group titled "Divisons"; Escape does nothing; the colour picker opens at its own default; the staff's right-click menu appears only after a first click | "Divisions"; Escape cancels; the colour panel opens on the swatch's colour; the right-click menu is there from the start | Spec F69, F70, F72's proposed fixes |
| Beat/Bar Detection and the modified flag (`crates/vixen-session/src/marks/beats.rs`) | Marks the sequence modified even when cancelled | Only when collections changed | Spec F67's proposed fix |
| Beat/Bar Detection progress | No Cancel; closing it crashes | A Cancel that stops the analysis and changes nothing but F68's default rule | Spec F74's guard |
| Mark undo after Bulk Effects Move or Add Marks to Effects (`apply_marks_snapshot` in `crates/vixen-session/src/sequence.rs`) | Upstream's mark undo holds only marks | Same, now: it restores each collection's marks by id. Before, the port's undo replaced the whole list, which would also undo panel changes that upstream can't undo | Matches upstream; listed because it changed shared code |
| Papagayo files (`crates/vixen-session/src/sequencer/papagayo.rs`) | A frame rate of 0 or less divides by zero and gives infinite or NaN times; a phoneme list shorter than its count, or a word line with too few fields, throws | The file fails to load with the importer's usual alert | Spec F87's guards. A good file reads exactly as before |
| Tools > LipSync > Papagayo Import, a file that can't be read | An unhandled exception | The "Papagayo Import" alert that mark import shows for the same file | Spec F88's guard |
| Lyric Converter, "Clear after conversion" (`crates/vixen-session/src/marks/lyric_converter.rs`) | Empties the text box as each word converts, so a convert given up at an unknown word has already lost the text | Empties it only once the conversion is delivered | Spec F80's proposed fix |
| Lyric Converter, Start Offset with no marks left | Keeps showing a mark time from a collection whose marks are gone, and converts against it | The Marks lists empty and Per Mark Collection greys | Spec F77, F78 |
| Lyric Converter window | Fixed size; the menu item does nothing while one is open | Resizable, so the text box can grow; choosing the item again brings it forward | Native window behaviour, spec F76. Same controls in the same order; the Marks labels sit inside the group rather than to its left |
| Text Conversion Failure dialog | The L button's caption reads "I" | "L" | Spec F82: a typo upstream |
| User phoneme dictionary (`user_dictionary`) | Its path is fixed at first use, so a profile switch keeps writing to the old profile's file; a failed write is an unhandled exception | Follows the open profile; a failed write is an alert and the mapping lasts until the app closes | Spec F85 |
| Adding a mark when there are collections and none is the default (`add_mark_at` in `crates/vixen-session/src/timeline/marks.rs`) | Asks whether to enable the active collection, and OK then does nothing | Doesn't ask; nothing is added, as upstream | The question has no effect upstream |
| Pointer while dragging a row's height (`row_mouse_move` in `crates/vixen-session/src/timeline/row_input.rs`) | Tested on each move against the label before it changes height, so a move of 3 pixels or more in one event shows the arrow until the next small move | Tested after the height changes: up-down for as long as the pointer tracks the bar, the arrow only at the 10 point floor or off the label, as upstream | A resize drag on the Mac keeps its resize pointer. Heights and everything else are the same |
| Tools > LipSync > Default Image Map (`AppCore::set_default_image_map` in `crates/vixen-session/src/marks/map_library.rs`) | The choice stays in memory until something else saves the module data | The module store is written at once | Spec F89's proposed fix: a quit could lose the choice |
| Image Maps, the default map | Its bold row is lost as soon as the list refreshes, and renaming or editing the default map leaves the library with no default until one is chosen again | The default's row stays bold, and the map stays the default through a rename and an edit | Spec F90, F95, F105 |
| Image Maps, New then Cancel | The entry goes but the folder of ten default pictures stays | The folder New made is removed too | Spec F91 |
| Image Maps, the changed flag | Each editor visit overwrites it, so a real change followed by a cancelled edit closes as unchanged and LipSync effects aren't rendered again | Sticky: once changed, it closes as changed | Spec F96 |
| Image Maps, Remove | The failure message reads "Unable to mapping data!"; selecting every map and removing empties the library | "Unable to remove mapping data!"; Remove is off while every map is selected | Spec F93 |
| Image Maps, renaming a map (`LipSyncLibrary::rename_map`) | Copies the folder then deletes the old one; a failed copy still renames the entry; a rename that changes only case deletes the pictures on a case-insensitive disk; a name with a path separator can write outside the LipSync folder | One folder move; a failed move leaves the map as it was; a case-only rename goes through a temporary name; names with a path separator, `.` and `..` are refused with "Can not set new name!" | Spec F118's guards |
| Image Map editor, a picture file that won't decode | An unhandled exception when the editor opens | That phoneme opens with no picture | Guard |
| Image Map editor, Edit | Writes `<phoneme>_tmp.bmp` in the map's folder, runs Microsoft Paint and blocks until it exits; a failed reload leaves a disposed bitmap behind | Writes a PNG in the system temporary folder, opens it in the Mac's default editor, and shows "Editing in <App>" with Done and Discard; Done reloads, and a file that won't load leaves the old picture | Spec F103: no Paint on a Mac, and a sheet shouldn't block the app |
| Image Map editor, "Map exists" | Yes overwrites, No keeps both | Overwrite, Keep Both, and a Cancel that returns to the editor with nothing written | A Mac confirmation always has a way back |
| Image Maps windows | Three modal windows; `<` and `>` text buttons; the picture box grows with the picture | Sheets on the owning window; chevron buttons; the picture fits a fixed 128 point square | Native. Same controls in the same places |
| LipSync effect naming a map that's gone (`resolve_lip_sync_mappings`, called from `register_sequence_session`) | Takes the default map's name when the effect first renders | Takes it when the sequence opens. Neither marks the sequence modified | The port renders from the stored values, so the name has to be settled before rendering. Same pictures on screen |
| Layers sharing one mixing filter (`split_shared_filter_instances` in `crates/vixen-io/src/sequence.rs`) | Every layer Add Layer makes while the Layers panel lives shares one filter instance: Configuration on one changes them all, and the file holds several surrogates naming one `InstanceId` with one data model | Each layer owns its settings. A new layer gets its own instance id; a file with a shared id is split when it opens, the first surrogate keeping the id and each later one getting a new id and its own copy of the data. The save then has one surrogate and one model per layer | Spec F117. Layers changing together is a surprise, and the port's one-model-per-layer save of a shared id would be a file Windows throws on. A file with no shared id is read and written exactly as before |

## Display Setup

From `docs/parity/display-setup.md`, where each has its reason.

| Where | Upstream | This port |
|---|---|---|
| OK with nothing changed | Writes and backs up the system files | Writes nothing and makes no backup |
| Property edits | Reach the system at once | Reach it at OK, so Cancel undoes them too |
| New controllers | Start as soon as they're added | Start at OK |
| Start and Stop while testing | Start the hardware | Touch hardware only when the app isn't muted. Cancel undoes what was started in that session |
| Paste on the patching canvas | | Centres what's pasted at the pointer |
| Filters not connected to anything | Upstream's top grid covers them | Drawn under connected ones |
| A filter whose line was deleted | | Stays on the canvas until the next element change or resize, so it can be connected again |
| OK after a filter was moved off a filter that later lost its own source | Deletes the moved filter too, though it is still connected, because the old parent's child list is never updated | Deletes only filters that have no source, and what hangs off them now |

## Preview setup

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| Opening the preview (`verify_preview_shape_locations` in `crates/vixen-preview/src/session.rs`) | `VerifyPreviewShapeLocations` removes any shape whose left, right, top and bottom are one number as "added by mistake". A Single light's bounds are always its one point, so every Single drawn where x equals y is deleted on the next open | Keeps Single lights; other shapes collapsed to one diagonal point are still removed | Upstream silently deletes a light the user placed. A Single off the preview is still brought home by the next check | Drop the `ShapeKind::Single` exclusion from the no-size check |

## Administration

The window-by-window decisions (Q1 to Q23) are in `docs/parity/administration.md` under "Quirk decisions", each with its reason. These came out of the review and aren't in that list.

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| `VixenApplicationData.xml` written by a newer Vixen (`crates/vixen-io/src/appdata.rs`) | Reads nothing from it (`given data version was too high`), and the next save writes an empty file at its own version, 3 | A file whose version is above 3 keeps its version and its unknown content. An older file saves as version 3, as upstream | Opening a profile here shouldn't strip what a newer Vixen wrote | Always write `CURRENT_APPLICATION_DATA_VERSION` in `save` |
| A profile lock naming this process's own id (`crates/vixen-io/src/lock.rs`) | Locked, for good: the process it names is running and is Vixen | Stale, and removed | It can only be left from an earlier life of the same id, or be a lock this run already holds | Remove the own-id branch in `check_profile_lock` |
| System > Logs (`crates/vixen-session/src/logging.rs`, `list_log_files`) | Lists every file in the logs folder | Leaves out files whose name starts with a dot | Finder's `.DS_Store` would otherwise be a menu item. Windows has no such file | Drop the `starts_with('.')` test |
| Check for Updates, GitHub's JSON (`crates/vixen-session/src/updates.rs`) | Json.NET's reading: member names match without regard to case, and numbers and strings convert to each other | Exact member names and types. Anything else is `GitHub update check returned malformed JSON.` | GitHub's API sends the exact names. Not copying a parser's leniency | |

## Hardware

| Where | Upstream | This port | Why |
|---|---|---|---|
| OpenDMX output | Goes through FTDI's Windows driver | Goes through the Mac's serial driver | No Mac version of that driver path. Not tried on real hardware |
| Serial: Mark or Space parity, 1.5 stop bits | Supported | Reports an error | macOS serial ports don't offer them |
| RDS HTTP(S) resource limits (`crates/vixen-output/native/rds_http.c`) | Framework response limits; accepts HTTP(S) | Supports HTTP(S) with normal certificate/hostname validation; rejects other schemes, limits body to 16 MiB, headers to 64 KiB and connect to 10 s | Bounds controller response work; 100 s overall deadline and redirect/auth behavior match the local HttpClient reference. See [RDS verification](review/rds-https.md) |

## Not modelled

| Where | Upstream | This port | Effect |
|---|---|---|---|
| Colour equality | `Color.White != Color.FromArgb(255,255,255)`, because .NET compares known-colour identity | Compares values | Confirmed State render impact: named `Color.Blue` on the default multiple-discrete RGB set falls back to red upstream, while the port uses blue. Value-only ARGB blue renders blue on both. Owner confirmed value-based colors on November 11, 2026. Two-case unchanged-C# probe in `tools/golden/states` reproduces this deliberate value-equality difference; core color identities are not modeled. See also `docs/audit/editor-pass.md` |

## Questions for the owner

These are built one way for now, and easy to change. The full list of quirk decisions for each window is in its `docs/parity/` file.

| Question | Built as | Where |
|---|---|---|
| Should the app look for updates by itself? Upstream checks at launch | It only checks when you choose Check for Updates, so it never uses the network unasked | `docs/parity/administration.md` Q1, F29 |
| What should Check for Updates point at? | Upstream's releases. Their installers are for Windows, and the Download button's tooltip says so | Q11 |
| A profile locked by another machine | Adds an `Open Anyway` button upstream doesn't have | Q3 |
| Opening a sequence that's already open | Focuses its window | Q22 |
| Export timing the first time you export on the Mac | Upstream's default of 50 ms, though every one of your real exports used 25 ms. Your own setting lives in `Settings.xml` on the Windows machine | `docs/spec/shows-scheduler-export.md` Q1 |
| The warning after each Falcon export that controllers lack universes | Only when Generate Controller File is ticked. Upstream shows it every time, which for your show means every export | Q2 |
| The web server on first launch | Implemented: enabled by default on port 8888, without authentication, on IPv4/IPv6 interfaces; saved disabled settings are respected. The UI test harness suppresses startup. Whether to change this upstream default remains a policy choice | Q3 |
| Bulk Move with clipping | As upstream: an effect clipped by the move stays in the sequence and keeps playing, though it's no longer shown in its row. I'd recommend changing this, since nothing on screen shows the effect is still there | `docs/parity/timeline.md` |
| Right-clicking a row while several are selected | As upstream: it collapses the selection to that row. Finder keeps the selection | `docs/parity/timeline.md` |

## Custom Prop Editor

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| Document dirty tracking and undo (`vixen-prop/editor.rs`) | Several graph/state edits lose dirty flags; no undo | One document revision and snapshot undo/redo, integrated with native UndoManager | Keeps real edits protected and makes editing recoverable | Replace revision tracking and remove snapshots |
| New/Open/close/quit (`VixenTools/PropEditorModel.swift`, `AppModel` close guards) | New/Open discard silently; close starts async save without awaiting it | Confirms replacement; Save/Don't Save/Cancel waits for validation and file completion; Cancel prevents window close or application shutdown | Prevents loss on save failure or cancelled panels | Remove replacement guard and await |
| xModel import (`vixen-prop/xmodel`, `PropXModel`) | Can replace a dirty document and can throw/allocate excessive memory for extreme geometry | Confirms replacement, keeps the current document on cancellation/error, rejects nonfinite coordinates, more than1million nodes, XML over128MiB, and raster canvases over64million pixels | Protects edits and turns source exceptions/resource exhaustion into an import error | Remove replacement and allocation guards |
| Vendor Browser (`vixen-prop/vendor`, `PropVendor`) | Switching vendors keeps the old product; blank email can throw; downloaded native props retain a temp path; some failures leave busy state active | Changing vendors clears the product, blank email is hidden, every downloaded document is untitled/dirty, failures preserve the source and clear busy state, byte imports need no temp file; native sheet has Cancel | Required spec fixes and native sheet cancellation | Restore stale selection, temp-path ownership and unchecked callbacks |
| Element Info Model Type (`vixen-prop/editor.rs`) | Property grid bypasses the resolver, allowing leaf Models and multiple explicit Models; State validation blocks Save later | Picker calls the resolver: leaf Models are rejected and choosing a Model demotes the previous one; its authored states remain attached and undo restores the previous role | Spec F18b permits enforcing the resolver instead of creating an invalid state | Assign the raw role without resolver checks |
| Order Reverse with gaps (`vixen-prop/editor.rs`) | Uses stored Order minus one as an insertion index; gaps after deletion can throw | Swaps selected list positions, then renumbers1…n | Required F38 correction; preserves unselected positions and works after deletion | Use stored Order as the insertion index |
| Tree/Order drag boundaries (`PropPanels.swift`, `vixen-prop/editor.rs`) | Shared drag context can move view models between Layout and Order lists; side drops can mix group/light child types | Distinct pasteboard contexts and source-table validation reject cross-list/cross-window Order drags; all tree drop positions enforce compatible child types | Avoids corrupting the tree/list graph and preserves model container rules | Share drag contexts and remove source/type validation |
| First canvas drag event (`vixen-prop/canvas.rs`) | First move only starts drag tracking; translation or band selection begins on the next move | Applies movement or band selection on the first native drag event | Keeps short/coalesced AppKit drags responsive; final delta is preserved even when only one drag event arrives | Add a start-only flag for both light and band drags |
| State canvas drag (`vixen-prop/canvas.rs`) | A stale selection mode can translate lights after state toggle | State mode only edits assignments, never positions | Required spec correction; assignment gestures must not move lights | Reuse previous non-state drag mode |
| Rename unchanged element (`vixen-prop/editor.rs`) | Counts the element itself as a name collision and adds a suffix | Excludes the renamed element | Required spec correction; accepting an unchanged name stays unchanged | Include the element in collision search |
| Custom Prop preferences (`PropPreferencesSheet`) | Selected Color edits State Preview Base Color by a misbound command | Separate native color controls edit their named setting and refresh all open prop windows | Required spec fix and immediate feedback | Bind Selected Color to the base color |
| Custom Prop text prompts (`PropPromptValidation`) | Blank names can be accepted | Verb buttons disable for blank input; State names also reject exact duplicates before acceptance | Required Mac prompt behavior | Remove prompt validation |

## Vixen 2 import

- Parse errors show `Import Failed` with the underlying error instead of disappearing silently. Empty audio names cause neither an audio picker nor a missing-audio alert. Old channel names use decoded XML text, preserving ampersands.
- Saving a channel map saves module configuration immediately. Either result from its editor resets selection, working mappings and the create/edit button. Deleted destinations are treated as unmapped. Arbitrary ARGB colors retain their swatch and stored value.
- Invalid RGB grouping is converted from a copy; saved mappings remain unchanged. RGB import colors retain .NET known-color identity when saved, so Windows can mix them. Named versus value-only colors are otherwise compared by ARGB, as elsewhere in this port.
- An imported audio track whose filename already exists in the profile gets an available numbered filename, preserving both files. Conversion progress runs in the background. The native destination mapping window also offers Map Selected and Delete to clear.


## Polygon Editor

| Where | Upstream | This port | Why | How to reverse |
|---|---|---|---|---|
| Empty TimeBased container | Opening throws while selecting frame zero | Opens with one empty frame at time zero | Required crash fix; an effect with all shapes removed remains editable | Reject empty input before opening |
| Closing a new polygon | A click near the first point can close one or two points | Requires at least three points | Avoids non-renderable polygons; explicitly permitted by F85 | Lower the close-point guard |
| Convert to Line in FreeForm | Writes the new coordinates into the first existing line | Updates the newly created line | Required wrong-shape fix | Address the first line instead |
| Ellipse scaling | Out-of-bounds correction has integer y division and can collapse the shape | Preserves floating-point ellipse geometry; interactive transforms reject out-of-bounds corners | Avoids corrupting editable geometry | Reintroduce the integer correction loop |
| Clipboard shortcuts | Toolbar only | Canvas supports ⌘X, ⌘C, ⌘V through a private system pasteboard format | Native Mac convention | Remove canvas responder shortcuts |
| Polygon window sizing | Primary display and Windows minimums | Owning display with the same 70% rule; native minimum dimensions keep toolbar and point controls usable | Mac window placement and control accessibility | Remove native minimums and use the primary screen |


## Remote web rendering and shutdown

- Experimental generic effect requests are rejected with HTTP 400 when checked leaf-count × rounded-up frame-count exceeds 16,000,000 samples. This bounds caller-controlled work before pixel renderers allocate buffers. It is a work guard, not a byte-accurate memory budget; ordinary indefinite On uses sparse Set Level intents and remains supported. Upstream imposes no corresponding render-work limit.
- Server shutdown signals effect rendering and sequence prerender polling to cancel and prevents a cancelled result from entering the live context. Synchronous sequence file/audio loading must still finish before its next cancellation check. A server released from its own callback uses a reaper to join that callback after it returns, avoiding a self-join deadlock; ordinary shutdown joins directly.
- Sequence listings use absolute canonical filenames even when the profile was opened with a relative directory, so a filename returned by the list can be passed directly back to Play. The existing profile containment and literal `+`/`%` handling remain in force.
