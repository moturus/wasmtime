#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
: "${RUSTUP_TOOLCHAIN:?set RUSTUP_TOOLCHAIN to the selected Motor Rust toolchain}"
export RUSTUP_TOOLCHAIN
cargo run --locked --release --manifest-path "$root/motor-tests/Cargo.toml" \
    --bin motor-compiler-policy-tests -j "${JOBS:-2}"
