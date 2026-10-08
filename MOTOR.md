# Motor OS port

The full `wasmtime` binary compiles core modules and components with Cranelift
and runs Pulley artifacts. `wasmtime-rt` omits Cranelift and also supplies the
immutable native ELF template. Both share the CLI and WASI p1/p2/p3 hosts. Motor
adapters use the existing native filesystem, networking, clock and entropy
APIs; this branch requires no core OS or toolchain changes.

Use real sibling upstream workspaces on these local Motor branches:

| Repository | Branch | Upstream base |
| --- | --- | --- |
| wasmtime | motor-48.0.1 | 7bac2c2775808aaec5d4aa5627a5e447b51102cf |
| target-lexicon | motor-0.13.5 | 6647bd7d681d3e218c88568d8285dfd8cae2dd97 |
| tokio | motor-1.51.1 | 98df02d7a4a638b3bc76a01f41966dc83c275103 |
| mio | motor-1.2.0 | ce39a6be2cc739165daaeb10cce609b9b77242ac |

`../motor-os` supplies the selected toolchain assembly and native crates. The
published Ring Motor commit is pinned in Cargo; upstream getrandom uses the
custom entropy callback in `wasmtime-wasi`. Neither needs a new fork. Wizer's
runtime-independent instrumentation lives in `crates/wizer` for Javy/Wasmi.
No manifest points to a prototype under `/tmp`.

Run `./motor-build.sh` (or `full` / `runtime`) from a Linux host with the Motor
assembly installed. `MOTOR_BUILD_DIR` selects a disk-backed output directory;
`JOBS` defaults to two. The full and runtime-only graphs use separate output
directories to prevent compiler features leaking into the runtime template.
The runtime build also produces the `package` development utility for native
ELF publication. Local branches need published source refs before normal Motor
image integration.

Run `./motor-test.sh` for compiler-policy and ELF publication regressions.
The Rust crates own their fixtures and have no runtime downloads. Native
filesystem checks live in `motor-host-tests`; build for Motor and run
`MOTOR_OS_CAPS=0x200 motor-wasi-tests`. Linear memories use owned lazy
reservations (96 MiB each, 128 MiB and four memories per process); qualify them
by precompiling `motor-runtime/fixtures/memory-*.wat` for `pulley64` with the
`compile` tool and running `MOTOR_OS_CAPS=0 memory-check memory-grow.cwasm
memory-too-large.cwasm` on Motor. Guarded fiber stacks use eight slots of up to 2 MiB,
each above a 4 KiB read-only guard, starting 16 MiB into Motor's custom
userspace region, above the process-data page that rt.vdso maps at its start;
nothing else maps there. A 2 MiB stack is charged 4,202,496 bytes because the
self-shared mapping has a read-write alias. Qualify with `stack-check normal`
and `stack-check overflow` on `motor-runtime/fixtures/stack-yield.wat`; the
overflow must kill only that process. `motor-runtime/fixtures/lifecycle.wat`
covers `run` stdio, exit status, traps and `-W timeout` (compile with
`--epoch`). Stores default to 64 instances, 16 tables and 32,768
table elements; `-W max-instances`, `max-tables` and `max-table-elements`
override them. The memory reservations above are a fixed host bound that
`-W max-memory-size` and `max-memories` can only lower; the
`motor-runtime/fixtures/limits-*.wat` modules exceed each default. Guest integration uses delivered binaries,
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
in Motor OS are proposed. The plan in `../motor-os/docs/plans/javy-wasmtime.md`
tracks remaining production integration, resource budgets and API coverage.
