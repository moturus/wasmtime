//! Motor's combined runtime CLI and immutable native ELF template.
use crate::common::RunTarget;
use clap::Parser;
use std::{ptr::NonNull, sync::Arc};
use wasmtime::{Config, Engine, Result, bail, ensure};

/// Internal module path used by the shared command and HTTP hosts.
pub const EMBEDDED_PATH: &str = "@motor-embedded";
use crate::motor_elf::{MAGIC, VERSION};
#[used]
#[unsafe(link_section = ".motor_version")]
static TEMPLATE_VERSION: [u8; VERSION.len()] = *VERSION;
const HEADER: usize = 4096;
const MAX_ARTIFACT: usize = 128 << 20;

core::arch::global_asm!(
    ".pushsection .motor_wasm,\"ax\",@progbits\n.balign 4096\n.global motor_wasm_start\nmotor_wasm_start:\n.space 4096,0\n.popsection\n"
);
unsafe extern "C" {
    static motor_wasm_start: [u8; HEADER];
}

fn header() -> &'static [u8; HEADER] {
    // Keep the publisher's version marker through linker section collection.
    std::hint::black_box(&TEMPLATE_VERSION);
    // The linker emits the whole empty section; the ELF publisher preserves it.
    unsafe { &motor_wasm_start }
}

/// Whether this process holds Motor's network capability.
pub(crate) fn has_network() -> bool {
    moto_sys::ProcessStaticPage::get().capabilities & moto_sys::caps::CAP_NET != 0
}

/// Reject excess process authority before parsing options or opening inputs.
pub fn check_authority() -> Result<()> {
    let caps = moto_sys::ProcessStaticPage::get().capabilities;
    ensure!(
        moto_sys::caps::ProcessRole::from_caps(caps) == moto_sys::caps::ProcessRole::None,
        "wasm tools require role None (caps={caps:#x}); use MOTOR_OS_CAPS=0, 0x100, 0x200 or 0x300"
    );
    ensure!(
        caps & !(moto_sys::caps::CAP_FS_WRITE | moto_sys::caps::CAP_NET) == 0,
        "wasm tools reject excess capabilities {caps:#x}; allowed masks: 0, 0x100, 0x200, 0x300"
    );
    Ok(())
}

/// An all-zero envelope selects ordinary runtime-only CLI operation.
pub fn populated() -> Result<bool> {
    if header().iter().all(|b| *b == 0) {
        return Ok(false);
    }
    ensure!(&header()[..16] == MAGIC, "invalid native ELF envelope");
    ensure!(
        &header()[24..24 + VERSION.len()] == VERSION,
        "native ELF version mismatch"
    );
    let _ = bounds()?;
    Ok(true)
}

fn bounds() -> Result<(usize, usize)> {
    let len = u64::from_le_bytes(header()[16..24].try_into().unwrap());
    let len = usize::try_from(len)?;
    ensure!(
        len > 0 && len <= MAX_ARTIFACT,
        "invalid native artifact length"
    );
    let start = (header().as_ptr() as usize)
        .checked_add(HEADER)
        .ok_or_else(|| wasmtime::format_err!("native address overflow"))?;
    ensure!(start == 0x1000_1000, "native template address mismatch");
    let end = start
        .checked_add(len)
        .ok_or_else(|| wasmtime::format_err!("native address overflow"))?;
    Ok((start, end))
}

struct CodeRange {
    start: usize,
    end: usize,
}
impl wasmtime::CustomCodeMemory for CodeRange {
    fn required_alignment(&self) -> usize {
        HEADER
    }
    fn publish_executable(&self, ptr: *const u8, len: usize) -> Result<()> {
        let start = ptr as usize;
        ensure!(
            start >= self.start && start.checked_add(len).is_some_and(|end| end <= self.end),
            "native code outside immutable ELF segment"
        );
        Ok(())
    }
    fn unpublish_executable(&self, ptr: *const u8, len: usize) -> Result<()> {
        self.publish_executable(ptr, len)
    }
}

/// Install the native code range only for a populated OS-loaded template.
pub(crate) fn configure(config: &mut Config) -> Result<()> {
    config.with_host_stack(Arc::new(crate::motor_stack::StackPool::default()));
    config.with_host_memory(Arc::new(crate::motor_memory::Reservations::default()));
    if populated()? {
        let (start, end) = bounds()?;
        config.target("x86_64-unknown-motor")?.motor_runtime();
        config.with_custom_code_memory(Some(Arc::new(CodeRange { start, end })));
    }
    Ok(())
}

/// Load only the immutable artifact produced by the matching ELF publisher.
pub(crate) fn load(engine: &Engine) -> Result<RunTarget> {
    ensure!(populated()?, "empty native ELF template");
    let (start, end) = bounds()?;
    // The trusted publisher validates the ELF segment and embeds the exact
    // serialized output. The OS owns this RX mapping until process exit.
    let bytes = unsafe { std::slice::from_raw_parts(start as *const u8, end - start) };
    let memory =
        NonNull::slice_from_raw_parts(NonNull::new(start as *mut u8).unwrap(), end - start);
    match Engine::detect_precompiled(bytes) {
        Some(wasmtime::Precompiled::Module) => Ok(RunTarget::Core(unsafe {
            wasmtime::Module::deserialize_raw(engine, memory)?
        })),
        Some(wasmtime::Precompiled::Component) => Ok(RunTarget::Component(unsafe {
            wasmtime::component::Component::deserialize_raw(engine, memory)?
        })),
        None => bail!("native ELF payload is not precompiled Wasm"),
    }
}

/// Use upstream command/HTTP parsing and hosts for the embedded artifact.
pub fn execute_embedded() -> Result<()> {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--serve") {
        args.remove(1);
        args.push("--allow-precompiled".into());
        args.push(EMBEDDED_PATH.into());
        crate::commands::ServeCommand::parse_from(args).execute()
    } else {
        let guest = args
            .iter()
            .position(|arg| arg == "--")
            .map(|index| args.split_off(index + 1));
        if guest.is_some() {
            args.pop();
        }
        args.push("--allow-precompiled".into());
        args.push(EMBEDDED_PATH.into());
        if let Some(guest) = guest {
            args.extend(guest);
        }
        crate::commands::RunCommand::parse_from(args).execute()
    }
}
