#!/bin/sh
# Builds the plugin and puts the program where vixen-plugin.json's entry.command expects it (bin/), so a linked
# development folder reloads once the new build is in place.
set -eu
cd "$(dirname "$0")"
cargo build --release
mkdir -p bin
cp target/release/{{CRATE}} bin/{{CRATE}}.new
mv bin/{{CRATE}}.new bin/{{CRATE}}
echo "Built bin/{{CRATE}}"
