# Motor OS port

The full `wasmtime` binary compiles core modules and components with Cranelift
and runs Pulley artifacts. `wasmtime-rt` omits Cranelift and also supplies the
immutable native ELF template. Both share the CLI and WASI p1/p2/p3 hosts. Motor
adapters use the existing native filesystem, networking, clock and entropy
APIs; this branch requires no core OS or toolchain changes.

This is branch `motor-48.0.1` of `moturus/wasmtime`, based on upstream
`7bac2c2775808aaec5d4aa5627a5e447b51102cf`. Its Motor dependencies are git
dependencies pinned by the checked-in lockfiles, never sibling checkouts:

| Dependency | Repository and branch | Upstream base |
| --- | --- | --- |
| target-lexicon | `moturus/target-lexicon` `motor-0.13.5` | 6647bd7d681d3e218c88568d8285dfd8cae2dd97 |
| tokio | `moturus/tokio` `motor-1.51.1` | 98df02d7a4a638b3bc76a01f41966dc83c275103 |
| mio | `moturus/mio` `motor-1.2.0` | ce39a6be2cc739165daaeb10cce609b9b77242ac |
| moto-rt, moto-sys, moto-io, moto-async, moto-ipc, moto-sys-io | `moturus/motor-os` `main` | (Motor OS itself) |

All Motor OS crates come from one `moturus/motor-os` revision; the lockfiles
may pin one ahead of `main` (`cargo update -p moto-rt --precise <rev>`), and
`cargo update -p moto-rt` moves them to the branch head together. `cargo update
-p tokio` (or `mio`, `target-lexicon`) takes a fork's new head. The published
Ring Motor commit is pinned in Cargo; upstream getrandom uses the custom entropy
callback in `wasmtime-wasi`. Neither needs a new fork. Wizer's
runtime-independent instrumentation lives in `crates/wizer` for Javy/Wasmi.

Run `./motor-build.sh` (or `full` / `runtime`) on a Linux host with
`RUSTUP_TOOLCHAIN` set to the Motor Rust toolchain and `MOTOR_SYSROOT` to its
assembly's sysroot (the `sysroot` directory beside the `images` directory that
Motor OS's `src/resolve-toolchain-assembly.sh --resolve` prints); Motor OS's
`src/build-motor-os.sh` exports both. `MOTOR_BUILD_DIR` selects a disk-backed
output directory; `JOBS` defaults to two. The full and runtime-only graphs use separate output
directories to prevent compiler features leaking into the runtime template.
The runtime build also produces the `package` development utility for native
ELF publication. The Motor OS tree builds the revision of this branch named in
its `src/build-addon-sources.sh`.

Run `./motor-test.sh` (same `RUSTUP_TOOLCHAIN`) for compiler-policy and ELF
publication regressions.
The Rust crates own their fixtures and have no runtime downloads. Native
filesystem checks live in `motor-host-tests`; build for Motor and run
`MOTOR_OS_CAPS=0x200 motor-wasi-tests`. Each linear memory owns one sparse lazy
reservation of its declared maximum, or 4 GiB without one, so only touched pages
use RAM; as on Linux there is no process-wide total or memory count. A 64-bit
memory cannot grow past 4 GiB: `memory.grow` returns -1 instead of moving it,
as upstream does with `-O memory-may-move=n`. Qualify the reservations
by precompiling `motor-runtime/fixtures/memory-*.wat` for `pulley64` with the
`compile` tool and running `MOTOR_OS_CAPS=0 memory-check memory-grow.cwasm
memory-too-large.cwasm` on Motor. Fiber stacks are Motor guarded heap maps
(`F_LAZY | F_GUARDED`): each sits between two pages that are never mapped, a
page is committed on first touch, and there is no limit on their number or
size, as with Wasmtime's own stacks on Linux. A pool keeps two of them per
engine for reuse, except for zeroed requests. Qualify with `stack-check normal`
and `stack-check overflow` on `motor-runtime/fixtures/stack-yield.wat`; the
overflow must kill only that process. `motor-runtime/fixtures/lifecycle.wat`
covers `run` stdio, exit status, traps, `-W timeout` (compile with
`--epoch`), and Ctrl-C and parent exit through its `ready-loop` export.
Stores default to 64 instances, 16 tables and 32,768 table elements; `-W max-instances`, `max-tables` and `max-table-elements`
override them. `-W max-memory-size` and `max-memories` work as upstream, below
the fixed 4 GiB per-memory bound. In `motor-runtime/fixtures`, `limits-tables.wat`
and `limits-elements.wat` exceed their defaults and `limits-memories.wat`
exceeds `-W max-memories=4`. Guest integration uses delivered binaries,
not a harness in the Motor source tree targeting temporary sources.

The Motor target selects serial memory-image coalescing, disables CoW and sets
the guaranteed dense-image allowance to zero, including cross compilation on
Linux. Native execution uses the populated template's existing executable ELF
mapping. Raw precompiled files are not an alternate native executable loader.
Tool processes require role None and reject capabilities other than filesystem
write and network access. WASI grants further restrict those capabilities.

Known port limits include native bind/listen ordering, unsupported bound TCP
connect and unspecified-address ephemeral binding, filesystem operations not
supported by native APIs, and upstream experimental WASI p3 behavior. These
limits are represented as errors or documented behavior; no networking changes
in Motor OS are proposed. Motor OS's `docs/plans/javy-wasmtime.md`
tracks remaining production integration, resource budgets and API coverage.
