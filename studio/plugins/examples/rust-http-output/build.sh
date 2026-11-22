#!/bin/sh
# Builds the plugin and puts the program where vixen-plugin.json's entry.command expects it (bin/), so a linked
# development folder reloads once the new build is in place.
set -eu
cd "$(dirname "$0")"
cargo build --release
mkdir -p bin
cp target/release/rust-http-output bin/rust-http-output.new
mv bin/rust-http-output.new bin/rust-http-output
echo "Built bin/rust-http-output"
