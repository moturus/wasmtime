use std::path::Path;
use wasmtime::{Result, ensure};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    #[cfg(target_os = "motor")]
    {
        let caps = moto_sys::ProcessStaticPage::get().capabilities;
        ensure!(
            moto_sys::caps::ProcessRole::from_caps(caps) == moto_sys::caps::ProcessRole::None
                && caps & !moto_sys::caps::CAP_FS_WRITE == 0,
            "publisher requires None and filesystem-only capabilities"
        );
    }
    ensure!(args.len() == 4, "package ARTIFACT TEMPLATE OUTPUT");
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(target_os = "motor")]
    match moto_rt::process::ctrl_c_register_handler() {
        Ok(sequence) => {
            let token = cancelled.clone();
            std::thread::Builder::new()
                .name("elf-cancel".into())
                .spawn(move || match moto_rt::process::ctrl_c_wait(sequence) {
                    Ok(_) => token.store(true, std::sync::atomic::Ordering::Release),
                    Err(error) => eprintln!("ELF cancellation listener failed: {error}"),
                })?;
        }
        Err(moto_rt::Error::NotFound) => {}
        Err(error) => wasmtime::bail!("ELF cancellation listener: {error}"),
    }
    wasmtime_cli::motor_elf::package(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
        || cancelled.load(std::sync::atomic::Ordering::Acquire),
    )?;
    println!("ELF publication PASS");
    Ok(())
}
