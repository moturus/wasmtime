fn main() -> std::io::Result<()> {
    #[cfg(target_os = "motor")]
    {
        let caps = moto_sys::ProcessStaticPage::get().capabilities;
        assert_eq!(caps, moto_sys::caps::CAP_FS_WRITE);
        filesystem::check()?;
        interleavings::check()?;
        println!("PASS Motor WASI filesystem adapter");
    }
    #[cfg(not(target_os = "motor"))]
    panic!("run this adapter test on Motor OS");
    #[cfg(target_os = "motor")]
    Ok(())
}
#[cfg(target_os = "motor")]
mod filesystem;
#[cfg(target_os = "motor")]
mod interleavings;

#[cfg(target_os = "motor")]
thread_local! {
    static RUNTIME_TLS: [std::cell::Cell<*mut u8>; 2] = const {
        [std::cell::Cell::new(std::ptr::null_mut()), std::cell::Cell::new(std::ptr::null_mut())]
    };
}
#[cfg(target_os = "motor")]
#[unsafe(no_mangle)]
pub extern "C" fn wasmtime_tls_get(slot: usize) -> *mut u8 {
    RUNTIME_TLS.with(|slots| slots[slot].get())
}
#[cfg(target_os = "motor")]
#[unsafe(no_mangle)]
pub extern "C" fn wasmtime_tls_set(slot: usize, pointer: *mut u8) {
    RUNTIME_TLS.with(|slots| slots[slot].set(pointer));
}
