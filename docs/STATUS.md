# Port status

This is an unofficial alpha port of Vixen to macOS by bguliano. It is not an official Vixen release and is not supported by the Vixen team. It has not been tested extensively: nearly all testing so far used one Mac and one real show. The current goal is reliable everyday use.

## Remaining usability work

- Validate the latest candidate through normal app launch and representative real-show workflows: open, edit, save, reopen, preview, playback and export.
- Fix crashes, data loss, broken workflows, and noticeable responsiveness, memory or audio synchronization problems.
- Promote the accepted build with a rollback copy and record remaining limitations.

November 20, 2026: hands-on checks with a real show covered startup, opening the largest sequence, playback with audio, the live preview, editing (select, copy/paste, undo, delete, move), save and reopen, FSEQ export, and the Preview designer. Three defects were fixed: the sequence editor now remembers its window size and position; closing a sequence editor now frees its memory (previously every open and close kept several hundred MB); and the Effect Editor's curve and gradient dialogs open again. New Sequence, the scheduler and Display Setup edits have not had hands-on checks yet.

Imperceptible audio/rendering differences are acceptable. Exact Windows parity, additional OS-version certification and Intel support are not requirements for this milestone. Always use a private copy of a show during testing.

The public test suite currently has 3,481 passing tests, zero failures and 10 ignored. Tests that needed the maintainer's private show were removed from this repository. No full-port completion is claimed. Private execution receipts and machine-specific session notes remain outside this public repository. Historical verification documents refer to those local receipts and may describe earlier builds or stricter parity goals.

The public source snapshot retains the current app implementation, synthetic fixtures, test tools, specifications and license notices. Local paths have been normalized to examples; private show files, recordings, keys, network setup scripts and build products are excluded.
