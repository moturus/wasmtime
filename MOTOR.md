# Motor OS port

This is `moturus/wasmtime` branch `motor-50.0.0`, based on upstream v50.0.0
(`ebe5e8df1f4e12c5fc2f5954926881d8d61367d8`). It supplies native Wasmtime
compilation, execution and WASI p1/p2/p3 hosts on Motor OS. Javy also uses this
branch's runtime-independent Wizer instrumentation with its Wasmi backend.

`wasmtime` compiles and runs raw Wasm modules/components, serves HTTP components
and compiles ordinary `.cwasm` artifacts. `wasmtime-rt` loads precompiled native
or Pulley artifacts and omits Cranelift. Both are built from the upstream CLI
package, using different features and separate output directories. Native
execution uses the host CPU by default; Pulley requires explicit selection.

## Build

Set `RUSTUP_TOOLCHAIN` to the Motor OS checkout's selected toolchain and
`MOTOR_SYSROOT` to `$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-motor/sysroot`.
The main tree's `src/build-motor-os.sh` supplies both values.

```sh
./motor-build.sh all       # full and runtime-only CLIs
./motor-build.sh full
./motor-build.sh runtime
```

`MOTOR_BUILD_DIR` defaults to `target/motor`; `JOBS` defaults to two. Outputs
are `full/x86_64-unknown-motor/release/wasmtime` and
`runtime/x86_64-unknown-motor/release/wasmtime-rt` beneath that directory.
Release settings are scoped to these Motor builds. The main tree installs both
commands in `/devtools/bin` and records their checksums and source revision.

Use the checked-in lockfiles. Required dependencies are the published
`moto-rt = "0.17"` and `moto-sys = "0.3.2"` crates, plus these Motor forks:

| Dependency | Source |
| --- | --- |
| target-lexicon | `moturus/target-lexicon`, branch `motor-0.13.5` |
| Tokio | `moturus/tokio`, branch `motor-1.51.1` |
| Mio | `moturus/mio`, branch `motor-1.2.0` |
| Ring | `moturus/ring`, commit `b1dad2579de791d0c31ad33300187e584ba6c268` |

The main tree pins this fork in `src/build-scripts/external-sources.sh`.
Owner publication precedes committing a dependent main-tree revision.

## Platform adaptations

Motor OS must include kernel commit `9b97640d` or equivalent self-executable
mapping support. Code is copied through a fresh writable alias of its executable
mapping, then that writable alias is revoked before execution. No native ELF
template, packaging command or custom artifact version is needed. In-place
code editing remains unsupported by this adapter.

Native code uses explicit bounds/trap checks and initializer coalescing without
copy-on-write. Each linear memory owns a lazy reservation up to its declared
maximum or the address/mapping limit (4 GiB for Wasm32, 1 TiB for memory64).
Growth does not copy the reservation or commit untouched pages. Fiber stacks
use guarded lazy mappings; two released stacks may be cached per engine.
Upstream store limits and p3 defaults apply, including explicit CLI overrides.

Both CLIs require role None and accept capabilities `0`, `0x100`, `0x200` or
`0x300`. WASI grants further restrict filesystem/network access. Native
filesystem primitives confine paths beneath each directory grant; directory
handles name paths while open files retain their native object. Socket
adapters preserve native bind/listen ordering and report unsupported native
operations. Clocks, randomness and runtime TLS slots use Motor APIs. Outgoing
TLS trusts `/system/cfg/ssl/ca-certificates.crt`.

## Maintained checks

`./motor-test.sh` runs the existing compiler/initializer regressions on the
Linux host using the selected toolchain. `./motor-build.sh tests` builds that
same binary for execution inside Motor OS; the main tree's installed Wasmtime
suite runs it as `engine-tests`.

The main tree's `src/tests/test-wasm.sh --suite wasmtime` covers the installed
native compile/run/serve workflow, TypeScript, precompiled loading, explicit
Pulley, WASI filesystem/networking, p3, lifecycle and limits. Its established
gates rebuild all guests and precompiled artifacts from this revision. The
used core fixtures remain under `motor-runtime/fixtures/`; that directory is
no longer a Cargo workspace. See Motor OS's [WebAssembly guide] and
[build and test instructions] for the installed tools and their checks.

[WebAssembly guide]: https://github.com/moturus/motor-os/blob/dev/docs/wasm.md
[build and test instructions]: https://github.com/moturus/motor-os/blob/dev/docs/build.md
