#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --locked --release --target wasm32-unknown-unknown --lib
wasm-bindgen --target web --out-dir site/app/pkg --out-name openmmd target/wasm32-unknown-unknown/release/openmmd.wasm
