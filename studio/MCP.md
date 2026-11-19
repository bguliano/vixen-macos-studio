# Vixen Studio MCP

## Status

Implemented: local bridge/in-app socket, shared live display undo, sequence authoring, real beat/bar analysis, inspection PNGs and FSEQ export. Only executable tools are advertised. The main UI branch must wire the new Studio FFI API into its panes. Legacy modal APIs remain compatible but must not be mixed with the live pane. This is not a claim of exhaustive native-editor parity; limitations are explicit below.

## Transport/process model

- vixen-studio-mcp --profile /absolute/profile speaks newline-delimited JSON-RPC on stdio. Logs stay off stdout.
- Prefer the profile-specific Unix socket under ~/Library/Application Support/Vixen Studio/. Directory 0700, socket 0600, same-user ownership, no symlink endpoints.
- Canonical-profile handshake prevents wrong-show attachment. Only a missing socket permits headless fallback. Existing stale/broken/insecure endpoints fail closed.
- Headless takes Vixen's exclusive .lock; the app recognizes the lowercase bridge as a live owner. No stale-lock stealing. Headless is always muted and starts no app services.
- VixenCore.start installs the server. Lower-level VixenCore.open is a test constructor; call studioStartMcp explicitly when using it. Shutdown rejects commands and drops the listener before releasing the profile lock.
- Max eight clients, 1 MiB requests, 8 MiB responses, 120-second idle reads, 30-second writes, bounded list pages. Same-user permissions are the trust boundary, not a sandbox against same-user malware.
- No HTTP listener, remote exposure, OAuth/token server or FPP network upload.

The synchronous adapter implements legacy MCP initialization (2024-11-05), tools, resources and prompts. Tasks, cancellation, subscriptions and HTTP are not advertised. The maintained official rmcp SDK (https://github.com/modelcontextprotocol/rust-sdk) was reviewed; it provides Tokio-based modern transport. The current adapter retains the app's threading model. Prefer rmcp before expanding protocol features.

## Commands and undo

vixen-session owns behavior; MCP adapts JSON-RPC. AppCore owns StudioDisplay, not a window. Display commands validate an isolated candidate through SetupSession algorithms, publish through AppCore::apply_display, then notify outside service/session locks. Failed commands do not partially publish.

Display, preview-prop and show/schedule edits share one profile undo stack (20 full snapshots). Agent descriptions end in (Agent). Stable UUIDs identify elements/controllers/shows; preview prop IDs and schedule indexes require expectedRevision. Stale edits fail before mutation. Explicit profile save retains undo. Existing system/module serializers are separate writes; a second-write failure reports that the first succeeded.

Sequences use the same registered Arc<Mutex<SequenceSession>> and UndoStack as UI windows. Agent effects/timing/parameters/layers/marks record native undo actions grouped with (Agent). StudioSettings adds reversible length/layer configuration. UI opening a saved agent document reuses the existing session; unsaved documents are obtained by sequence ID. Rendering uses the real engine schedule.

The show window's Display pane edits in a Display Setup working copy and publishes it with studioPublishDisplaySetup(session:expectedRevision:cleanup:): published exactly as a command's validated candidate is, one undo entry per publication (labels such as Edit Elements, Patching, Edit Controllers), refused when the display moved past expectedRevision or while non-muted execution is open (studioDisplayEditBlock). Legacy openDisplaySetup/commit/discard remains unchanged for compatibility. External/legacy display publication invalidates old Studio history on the next command rather than undoing unrelated edits. The new sidebar pane must use Studio commands/snapshots only.

## Swift integration contract

Generated VixenCore methods:

- studioDisplayCommand(command:argumentsJson:) -> JSON {changed,revision,result}; UI origin implicit. MCP supplies agent origin internally, never from JSON.
- studioDisplayCommandCatalog() and studioSequenceCommandCatalog() return exact schemas.
- studioDisplaySnapshot() -> {revision,elements,controllers}, fetched in bulk; studioDisplayHistory() returns newest-first undo/redo descriptions.
- studioSequenceCommand(command:argumentsJson:) calls the same authoring functions as MCP.
- studioOpenSequenceSession(sequenceId:) returns the shared SequencerSession for an agent-created/opened document.
- studioObserveDisplay(observer:) / studioStopObservingDisplay(): one observer per core. Callback occurs on a command thread, outside engine/session locks; dispatch to MainActor.
- studioDisplayEvents(afterRevision:) returns up to 100 events. Event revision is NOT the display revision used for expectedRevision.
- studioAgentSnapshot() returns authoritative clients, active tools and bounded recent outcomes; use on attachment/reconnect/gaps.
- studioStartMcp() throws on startup failure; studioMcpIsRunning() reports installed server state.
- studioStopMcp() drops only the listener (removing its socket) for exits that skip shutdown. The UI test harness calls it from `UITestHarness.beforeExit` before `quit` exits, so consecutive script runs on one profile each get a live server.
- studioAllowAgentsLiveOutput() / studioSetAllowAgentsLiveOutput(allowed:) are local-UI-only. Permission resets OFF each launch. Revocation stops agent-started controllers/test context. MCP has no setter.

StudioAgentActivity.observe(core:) is an @Observable MainActor adapter, built as a separate Swift module. It loads snapshots on attachment/callback, tracks clients/current tools/recent outcomes and exposes changedDocumentId. The coordinator owns toolbar/pane presentation. No existing Swift views were restyled here.

Events: clientConnected/clientDisconnected (command holds opaque client ID), commandStarted/commandFinished/commandFailed, displayChanged, documentChanged (documentId), profileSaved, outputChanged. No arguments/audio paths in activity events. Preview edits set the existing preview reload flag. Refresh bulk pane snapshots on changes, never per row.

## Implemented surfaces

- Profile/status: identity/version/safety, sequence/media inventory, confirmed save. Profile selection is a launch argument, not silent app switching.
- Display: elements/groups CRUD, native arch/group/megatree/matrix templates, RGB/single-color handling, reverse wire order, undo/redo.
- Controllers: types/list/create/rename/delete, DDP/E1.31 IPv4-unicast configuration/auto universes, patch/unpatch, monitor, separately gated start/stop/test pattern.
- Preview: preview IDs/linked nodes, native prop/property inventory, common shape creation, primitive property edits, element linking, inspection PNG.
- Sequences: create with audio/open/status/save/length; effects list/add/delete/timing/copy/parameters; layers list/create/rename/delete/assignment; marks/undo/redo/render; guarded play/pause/stop/seek.
- Music: native beat/bar tracker, tempo estimate, 100ms RMS energy, explicitly labelled onset/section-change heuristics. Authored phrase/word/phoneme marks supported.
- Shows/schedules: list, sequential show create/rename/delete, disabled schedule drafts and confirmed deletion. No program-launch actions or remote activation.
- Diagnostics: unpatched elements, unused outputs, invalid patches/configurations, overlapping universes, open-sequence missing media/targets/native color corrections, bounded log tail, actual engine/controller state.
- Export: confirmed local uncompressed FSEQ v2 at 50ms; no upload.
- Resource studio://contract and build_show workflow prompt.

Shared structural schema validation enforces types/required fields/ranges, followed by native domain validation. Raw XML parameters are never accepted. Strings/lists/numeric work are bounded.

## Safety

Delete/unpatch/save/overwrite/export and output commands require explicit confirmation. Default-off local output permission cannot be changed by MCP. VIXEN_MUTE overrides startup/test/transport. Headless always stays muted. Agents cannot implicitly open execution and start every controller: open execution locally first; output_start addresses one E1.31/DDP controller. output_stop remains usable after permission revocation.

Display edits are rejected while non-muted execution is open because graph publication may restart controllers. Edits to a live non-muted sequence require stopping it first. Non-muted playback/seek requires local permission and confirmation. Analysis/render/export never plays sound or sends controller packets.

Writes stay within selected-profile Sequence/Export/config directories. Filename-only outputs, canonical parent checks, symlink rejection (including backup/temp trees), and exclusive save temporary files protect the original archive. Media may be a read-only symlink; no tool writes Media. The bridge refuses the known original-show archive. Tests use disposable copies and VIXEN_MUTE=1, never real output.

## Known limits

- No automatic lyrics transcription/phoneme alignment. Bars assume 4/4. Onset/section candidates are energy heuristics, not semantic song structure.
- PNG uses real geometry/links/schedule/color conversion but CPU inspection rasterization: no background artwork, fixture beams or GPU antialiasing; max 2048 dimensions.
- Common primitives/enums/colors/RGB/curves/gradients/nested records supported. Font, gradient-level pairs, raw XML and file-path parameter selectors remain rejected with rejecting schemas. Layer creation currently uses highest-value mixing.
- Controller editing covers IPv4 unicast, not every serial/specialist dialog or manual multicast/universe layout. Real hardware startup/test is deliberately untested.
- Schedule creation is disabled until local review; no program-launch actions or silent profile switching.
- Preview edits must not race an old private native-designer working copy. Redesigned UI event/reload integration remains the coordinator's lane.
- Undo is bounded by count, not bytes; huge profiles may be memory-intensive. Audio analysis: 100 MiB input/15 minutes. Templates: 4096 leaves. Long tasks have no MCP cancellation yet.
- A socket left by a crash or force-quit blocks the next launch of that profile: the app logs "Could not start MCP server … Address already in use" and Settings ▸ Agents shows Not running, and the bridge refuses the endpoint rather than falling back to headless. Remove the file named in the log by hand. See the proposal below.

## Proposal: stale-socket recovery (not implemented)

Status: proposed for owner review. The current rule (stale endpoints fail closed, no stale-lock stealing) stays in force until this is approved.

Recover a leftover endpoint only when it provably has no owner, in one place used by both the app's `SocketServer::start` and the headless bridge's fallback:

1. The caller already holds the profile's exclusive Vixen `.lock` (the app takes it at startup; headless takes it before serving). No lock, no recovery: another Vixen may own the profile. The `.lock` itself is never stolen.
2. The socket directory and file pass `secure()` unchanged: same uid, directory 0700 and not a symlink, file is a socket, mode 0600, not a symlink.
3. `connect()` to the endpoint returns `ECONNREFUSED`. Any other result fails closed as today: a successful connection means a live server, and timeouts, `EACCES`, `ENOENT` races or other errors keep the existing error.
4. Re-check with `symlink_metadata` that the device and inode match what step 2 inspected, unlink that path once, then `bind`. If `bind` still fails, report it as today. No retry loops.
5. Log one warning naming the removed path, for example `[Vixen.Studio.MCP] Removed stale MCP socket <path> (no listener; profile lock held)`.

Why it is safe: socket paths are per profile, so a refused socket for a profile whose exclusive lock this process holds can only belong to a dead process. The same-user check keeps the trust boundary where it is now. The device and inode recheck narrows the time-of-check window to a same-user process deliberately racing the unlink, which the current model already treats as out of scope. Nothing is removed for a profile this process doesn't own.

Tests to add with it: start the app over a refused socket left by a killed process and expect a running server and one log line; a symlinked, wrong-mode or foreign-uid endpoint stays fail-closed; a live listener is never removed; and with no lock held (another process owns the profile) nothing is touched.

See MCP-CLIENTS.md for setup and MCP-INTEGRATION.md for verification/marked-file handoff. Use tools/list for exact schemas.
