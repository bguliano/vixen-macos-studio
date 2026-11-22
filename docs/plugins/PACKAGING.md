# Packaging and distribution

## The package: `.vixenplugin`

A `.vixenplugin` file is a zip of the plugin folder with `vixen-plugin.json` at its root. Make one with:

```sh
vixen-studio-mcp plugin pack path/to/my-plugin                  # writes <id>-<version>.vixenplugin next to the folder
vixen-studio-mcp plugin pack path/to/my-plugin --out dist --force
```

`pack` validates the manifest first and writes `<id>-<version>.vixenplugin` plus `<that>.sha256`. It is deterministic
(files in name order, fixed timestamps, only the executable bit of each mode), so the same folder always gives the same
bytes and checksum. It leaves out `.git`, caches, `target/` and earlier packages, refuses symlinks, and refuses a
package whose in-folder `entry.command` is missing (build a Rust plugin's `bin/` first).

Users install it with Settings ▸ Plugins ▸ Install from File… (or by dropping it on the list), which shows what the
plugin adds and its permissions before copying anything. Agents use `plugin_install {path, confirm: true}` once the user
allows agents to manage plugins. A folder installs the same way as a package.

Installed plugins live in `~/Library/Application Support/Vixen Studio/Plugins/<id>/<version>/`. One version of each
plugin is installed at a time: installing another replaces it. Uninstalling keeps the plugin's data folder and log.

## Versions

- `version` is your plugin's semantic version. Bump **patch** for fixes, **minor** for additions that keep existing
  settings working, **major** when a show's saved settings or behaviour would change.
- Shows record what they need: each save writes `StudioPlugins.json` with `^major.minor` of the version each controller
  was set up with (`^0.minor.patch` while you're at 0.x). A show set up with 1.2.0 accepts 1.2.x and 1.9.0 but says
  "Needs plugin … ^1.2 — version 1.1.0 is installed" with 1.1.0, and won't take 2.0.0.
- **Keep `typeId` and `id` forever.** Shows store them. A new major version keeps reading the settings older versions
  wrote: normalise them in `controller.create` (answer the upgraded `config`) instead of refusing them.
- `apiVersion`: `^1.0` works with every Studio speaking plugin API 1.x. API minor versions only add; a breaking change
  would be API 2.0, with a documented migration.

## Native programs: signing and notarising

Python and Node plugins ship as source and run with the user's `python3` or `node`. A compiled plugin (Rust, Swift, Go)
ships a program, and macOS checks programs that arrive from the internet:

- Studio clears the quarantine flag on what the user chose to install, so an unsigned program in a package installed
  through Studio runs. A program copied in any other way may be blocked by Gatekeeper.
- To distribute a compiled plugin widely, sign it with your Developer ID and notarise it before packing:

  ```sh
  codesign --force --options runtime --timestamp --sign "Developer ID Application: Your Name (TEAMID)" bin/my-plugin
  ditto -c -k bin/my-plugin my-plugin.zip
  xcrun notarytool submit my-plugin.zip --keychain-profile my-notary-profile --wait
  ```

  (A bare executable can't be stapled; notarisation is checked online the first time it runs.)
- Build for both architectures (`aarch64-apple-darwin` and `x86_64-apple-darwin`, joined with `lipo`) if you want Intel
  Macs to run it.

## Distribution

Publish the `.vixenplugin` and its `.sha256` wherever you like, for example as GitHub release assets. Put the plugin in
its own repository (Studio's repository only carries the generic examples) with a README saying what devices it
drives, what its permissions are for, and how to report problems. Users can check a download against your `.sha256`
with `shasum -a 256 <file>.vixenplugin`; Studio records the same checksum in `Plugins/registry.json` when it installs
the package.
