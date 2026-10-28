//! Host policy shared by Motor's full and runtime-only CLIs.
use wasmtime::{Result, ensure};

/// Whether this process holds Motor's network capability.
#[cfg(any(feature = "run", feature = "wizer"))]
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

/// Use owned lazy memories and guarded fibers in both command and HTTP hosts.
#[cfg(any(feature = "run", feature = "wizer"))]
pub(crate) fn configure(config: &mut wasmtime::Config) -> Result<()> {
    use std::sync::Arc;
    config.with_host_stack(Arc::new(crate::motor_stack::StackPool::default()));
    config.with_host_memory(Arc::new(crate::motor_memory::Reservations::default()));
    Ok(())
}
