#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
motor=$(cd -- "$root/../motor-os" && pwd)
export RUSTUP_TOOLCHAIN=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$motor/rust-toolchain.toml")
assembly_images=$("$motor/src/resolve-toolchain-assembly.sh" --resolve)
assembly_sysroot=${assembly_images%/images}/sysroot
export CC_x86_64_unknown_motor=$assembly_sysroot/bin/motor-clang
export CARGO_TARGET_X86_64_UNKNOWN_MOTOR_LINKER=$assembly_sysroot/bin/motor-clang
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
