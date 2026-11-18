# Vixen Studio MCP contract

## Design and implementation milestones
M0 defines the contract; features below are planned until listed as implemented.

The bridge `vixen-studio-mcp --profile PATH` speaks newline-delimited MCP JSON-RPC on stdin/stdout (logs only on stderr). Prefer the in-app Unix socket under `~/Library/Application Support/Vixen Studio/` (directory 0700, socket 0600). No TCP listener or remote exposure. A profile identity handshake must prevent attaching to the wrong show. Headless fallback must acquire the same exclusive profile lock as the app; no fallback on a busy or inaccessible socket. Headless is always muted, with live hardware disabled.

## Shared commands and events
`vixen-session` owns validation, mutations, undo and render scheduling. The MCP crate is transport/schema adaptation only. Agent requests must operate on the same Arc-backed sequence/setup sessions as Swift; independently reopening an active document is forbidden. Each successful edit records one atomic undo action labelled `… (Agent)`. Save and destructive commands require `confirm: true`; ordinary non-destructive edits do not. A failed command must not publish a revision or partial edit.

The Studio-only FFI event interface carries monotonically increasing revision, kind (`clientConnected`, `clientDisconnected`, `commandStarted`, `commandFinished`, `documentChanged`, `displayChanged`, `outputChanged`), client ID, command, document ID, summary and success. UI callbacks marshal to MainActor; callbacks never execute with an engine lock held. Clients obtain a snapshot on attachment and after a revision gap. Activity is bounded; no audio paths or tool arguments in logs by default. `app/Sources/StudioAgent` exposes an @Observable MainActor activity model (connected clients, current tool, recent actions). UI owns toolbar styling.

## Capability inventory
- profile: status, app info, profile selection, sequences/media, explicit save
- display: elements/groups CRUD, templates, colors/properties, wire order
- controllers: CRUD/types, outputs/universes, patch/unpatch/report
- live: separate start/stop/test commands; both explicit confirmation and local setting required
- preview: props, linking, frame PNG rendering
- sequence: create/open/length/audio, effects CRUD/parameters/catalog, layers, copy/paste, marks, undo/redo, render status, muted transport
- music: tempo/beats/bars/onsets/energy; beat marks
- shows/schedules: list and edit
- diagnostics: patches, overlapping universes, missing media, incompatible colors, logs and controller stats
- export: explicit confirmed fseq output

Tools have closed object JSON schemas, stable IDs, clear units (milliseconds unless specified), bounded lists and readOnly/destructive annotations. Advertise only executable tools, not placeholders. MCP resources expose this contract and profile summaries; workflow prompts describe inspect → analyze → construct → render → validate → confirmed save/export.

## Safety
Allow agents to control live output defaults OFF, is set only by local UI, and cannot be enabled by MCP. VIXEN_MUTE=1 overrides all settings. Neither analysis nor playback starts controllers. All writes stay within the selected profile or an explicitly approved export destination; canonicalize paths and reject symlink escapes (especially Media). Original show is read-only and never opened for writes. Destructive edits/overwrites require explicit confirmation. Socket peers are local same-user processes; file permissions are the trust boundary, not protection against other same-user malware. Bound messages, responses and work; disconnect releases client activity state.

## UI integration requirements
Do not fork sessions for MCP. App starts/stops the server with its core lifecycle and exposes the shared activity/event adapter. Existing UI session ownership and undo handling must be unified before live mutation tools are advertised. UI may render event snapshots without implementing business rules.

## Implemented scope (M1 foundation)
The initial implementation is a locked, read-only headless snapshot with seven tools, resources and a workflow prompt. It deliberately does not advertise mutations, socket attachment, analysis, render/export or playback. Existing session APIs interpret display/patch data. Stdio has 1 MiB input and 8 MiB response limits, strict tool arguments and 500-item pages. No app core is booted and no media is decoded.

Integration prerequisite discovered: DisplaySetupSession privately owns a RwLock<SetupSession>, while open sequencers register Arc<Mutex<SequenceSession>> with AppCore. Display edits lack a shared session-level undo stack. M2 must unify setup ownership and add shared display undo first; an agent-only working copy is not live parity. Sequence actions can use UndoAction::Group to label shared undo entries with (Agent). The profile-lock recognizer must recognize the lowercase bridge or the app treats its live lock as stale.

Preflight: no existing MCP dependency in this repository. The narrow synchronous read-only adapter uses existing serde_json instead of introducing an async runtime. Before concurrent sessions/cancellation/HTTP, evaluate the maintained official Rust MCP SDK (rmcp) and migrate transport rather than expanding a bespoke stack.

StudioAgentActivity is a standalone @Observable MainActor adapter with bounded snapshot replacement. It is not yet registered in Package.swift or connected to FFI; do not show a connected indicator until a real server snapshot arrives. No existing views were changed.
