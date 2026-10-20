#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# The Motor OS build exports the selected toolchain and its assembly's sysroot
# (src/build-motor-os.sh). Stand-alone: RUSTUP_TOOLCHAIN is the channel in the
# Motor OS checkout's rust-toolchain.toml, and MOTOR_SYSROOT is the `sysroot`
# directory beside the `images` directory that its
# src/resolve-toolchain-assembly.sh --resolve prints.
: "${RUSTUP_TOOLCHAIN:?set RUSTUP_TOOLCHAIN to the selected Motor Rust toolchain}"
: "${MOTOR_SYSROOT:?set MOTOR_SYSROOT to the Motor toolchain assembly sysroot}"
export RUSTUP_TOOLCHAIN
export CC_x86_64_unknown_motor=$MOTOR_SYSROOT/bin/motor-clang
export CARGO_TARGET_X86_64_UNKNOWN_MOTOR_LINKER=$MOTOR_SYSROOT/bin/motor-clang
output=${MOTOR_BUILD_DIR:-$root/target/motor}
case ${1:-all} in
    full|all)
        CARGO_TARGET_DIR="$output/full" cargo build --locked --manifest-path "$root/Cargo.toml" \
            --release --target x86_64-unknown-motor --bin wasmtime --no-default-features \
            --features run,serve,compile,cranelift,pulley,component-model-async,gc,gc-drc,disable-logging \
            -j "${JOBS:-2}"
        ;;
    runtime) ;;
    *) echo 'usage: motor-build.sh [full|runtime|all]' >&2; exit 2 ;;
esac
if [[ ${1:-all} != full ]]; then
    CARGO_TARGET_DIR="$output/runtime" cargo build --locked --manifest-path "$root/motor-runtime/Cargo.toml" \
        --release --target x86_64-unknown-motor --bin wasmtime-rt --bin package -j "${JOBS:-2}"
fi
