#!/usr/bin/env bash
# Build the lightstone-wasm crate and generate JS bindings into web/src/lib/wasm/.
# Requires: rustup target wasm32-unknown-unknown, wasm-bindgen-cli 0.2.130.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/.." && pwd)"
cargo build --release -p lightstone-wasm --target wasm32-unknown-unknown --manifest-path "$root/Cargo.toml"
wasm-bindgen --target web --out-dir "$here/src/lib/wasm" \
  "$root/target/wasm32-unknown-unknown/release/lightstone_wasm.wasm"
echo "WASM bindings written to $here/src/lib/wasm"
