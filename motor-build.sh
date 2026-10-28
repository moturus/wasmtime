#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
: "${RUSTUP_TOOLCHAIN:?set RUSTUP_TOOLCHAIN to the selected Motor Rust toolchain}"
: "${MOTOR_SYSROOT:?set MOTOR_SYSROOT to the selected Motor toolchain sysroot}"
export RUSTUP_TOOLCHAIN
export CC_x86_64_unknown_motor=$MOTOR_SYSROOT/bin/motor-clang
export CARGO_TARGET_X86_64_UNKNOWN_MOTOR_LINKER=$MOTOR_SYSROOT/bin/motor-clang
# Keep the delivered Motor settings local to these builds.
export CARGO_PROFILE_RELEASE_PANIC=abort
export CARGO_PROFILE_RELEASE_LTO=fat
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
export CARGO_PROFILE_RELEASE_STRIP=true
output=${MOTOR_BUILD_DIR:-$root/target/motor}
features=run,serve,pulley,component-model-async,gc,gc-drc,disable-logging,clap/default
build_cli() {
    CARGO_TARGET_DIR="$output/$1" cargo build --locked --manifest-path "$root/Cargo.toml" \
        --release --target x86_64-unknown-motor --bin wasmtime --no-default-features \
        --features "$features$2" -j "${JOBS:-2}"
}
case ${1:-all} in
    full|all) build_cli full ,compile,cranelift,wat ;;
    runtime) ;;
    tests)
        CARGO_TARGET_DIR="$output/full" cargo build --locked --manifest-path "$root/motor-tests/Cargo.toml" \
            --release --target x86_64-unknown-motor --bin motor-compiler-policy-tests -j "${JOBS:-2}"
        exit
        ;;
    *) echo 'usage: motor-build.sh [full|runtime|tests|all]' >&2; exit 2 ;;
esac
if [[ ${1:-all} != full ]]; then
    build_cli runtime ''
    cp "$output/runtime/x86_64-unknown-motor/release/wasmtime" \
        "$output/runtime/x86_64-unknown-motor/release/wasmtime-rt"
fi
