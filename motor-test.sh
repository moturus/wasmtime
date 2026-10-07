#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
export RUSTUP_TOOLCHAIN=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/../motor-os/rust-toolchain.toml")
cargo run --locked --release --manifest-path "$root/motor-tests/Cargo.toml" \
    --bin motor-compiler-policy-tests -j "${JOBS:-2}"
cargo test --locked --manifest-path "$root/motor-runtime/Cargo.toml" --test elf -j "${JOBS:-2}"
