//! Host policy shared by Motor's full and runtime-only CLIs.

/// Whether this process holds Motor's network capability.
#[cfg(any(feature = "run", feature = "wizer"))]
pub(crate) fn has_network() -> bool {
    moto_sys::ProcessStaticPage::get().capabilities & moto_sys::caps::CAP_NET != 0
}

/// Use owned lazy memories and guarded fibers in both command and HTTP hosts.
#[cfg(any(feature = "run", feature = "wizer"))]
pub(crate) fn configure(config: &mut wasmtime::Config) -> wasmtime::Result<()> {
    use std::sync::Arc;
    config.with_host_stack(Arc::new(crate::motor_stack::StackPool::default()));
    config.with_host_memory(Arc::new(crate::motor_memory::Reservations::default()));
    Ok(())
}
