# Vixen Studio

> **An unofficial, community-made macOS edition of Vixen Lights by [bguliano](https://github.com/bguliano).**
> It is not made, endorsed or supported by the Vixen Lights team. Please report problems
> [here](https://github.com/bguliano/vixen-macos-studio/issues), not to the Vixen team.

Vixen Studio is a redesigned take on [the unofficial macOS port of Vixen 3](https://github.com/bguliano/vixen-macos-port).
The port stays faithful to Windows Vixen; Studio keeps the same Rust engine and show files but rebuilds the app around
modern Mac design and workflows, and lets AI agents operate it through MCP.

| | [Vanilla port](https://github.com/bguliano/vixen-macos-port) | Vixen Studio |
|---|---|---|
| Interface | Matches Windows Vixen's layout | Redesigned for macOS: one show window, sidebars, inspectors, native toolbars |
| Workflows | Same as Windows Vixen | Reworked where Vixen's are confusing or slow |
| Show files | Vixen 3.13 format | The same format: shows move freely between both apps and Windows Vixen |
| AI agents | — | Full control through a built-in MCP server |

Both apps can be installed side by side.

## Status: early alpha

Studio is under active redesign and inherits the port's alpha status. **Back up your show folder before opening it**, and
preferably work on a copy. It runs on **Apple Silicon Macs with macOS 26 or later**.

## Build from source

You need Xcode 26 or later and Rust from [rustup.rs](https://rustup.rs). Then:

```sh
git clone --branch studio --recurse-submodules --shallow-submodules https://github.com/bguliano/vixen-macos-studio.git
cd vixen-macos-studio
scripts/build_app.sh release
```

The first build takes 5–25 minutes. The app is written to `build/Vixen.app`.

## How Studio relates to the vanilla port

Studio carries the port's full history and takes its changes by merging, so fixes made in the port reach Studio without
being redone. Studio's own changes to shared code are marked so a merge can't silently undo them. See
[STUDIO.md](STUDIO.md).

## Credits

- **Vixen Lights** — the original Windows application, its show format and its effects — is the work of
  [the Vixen team](https://github.com/VixenLights/Vixen) and its contributors since 2011. Their commit history is kept
  in this repository, which is why they appear in its contributor list. They aren't involved in this edition and
  aren't responsible for it.
- **The macOS port** ([vixen-macos-port](https://github.com/bguliano/vixen-macos-port)) — the Rust engine and native
  Mac app that replace the Windows/.NET code — and **Vixen Studio** — the redesigned interface, workflows and built-in
  MCP server — are by [Braden Guliano](https://github.com/bguliano).

## Licenses and attribution

Vixen is created by the Vixen team; its copyright and license are retained in [License.txt](License.txt) and the upstream
submodule. This app also includes components under other licenses, including GPL-2.0-or-later analysis code. Review
[the component license inventory](docs/LICENSES.md) before redistributing builds.
