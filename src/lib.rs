//! The Wasmtime command line interface (CLI) crate.
//!
//! This crate implements the Wasmtime command line tools.

#![deny(missing_docs)]

pub mod commands;

#[cfg(any(feature = "run", feature = "wizer"))]
pub(crate) mod common;

#[cfg(any(feature = "objdump", all(feature = "hot-blocks", target_os = "linux")))]
pub(crate) mod disas;

#[cfg(target_os = "motor")]
thread_local! {
    static MOTOR_WASMTIME_TLS: [std::cell::Cell<*mut u8>; 2] = const {
        [std::cell::Cell::new(std::ptr::null_mut()), std::cell::Cell::new(std::ptr::null_mut())]
    };
}

/// Motor's per-thread Wasmtime runtime slots.
#[cfg(target_os = "motor")]
#[unsafe(no_mangle)]
pub extern "C" fn wasmtime_tls_get(slot: usize) -> *mut u8 {
    MOTOR_WASMTIME_TLS.with(|slots| slots[slot].get())
}

/// Update a Motor per-thread Wasmtime runtime slot.
#[cfg(target_os = "motor")]
#[unsafe(no_mangle)]
pub extern "C" fn wasmtime_tls_set(slot: usize, pointer: *mut u8) {
    MOTOR_WASMTIME_TLS.with(|slots| slots[slot].set(pointer));
}

#[cfg(all(target_os = "motor", feature = "motor-template"))]
pub mod motor;
#[cfg(all(target_os = "motor", feature = "motor-template"))]
pub mod motor_memory;
#[cfg(all(target_os = "motor", feature = "motor-template"))]
pub mod motor_stack;

#[cfg(feature = "motor-template")]
pub mod motor_elf;
