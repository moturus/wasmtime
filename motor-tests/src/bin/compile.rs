use wasmtime::{Config, Engine, Result, ensure};

#[cfg(target_os = "motor")]
thread_local! {
    static TLS: [std::cell::Cell<*mut u8>; 2] = const {
        [std::cell::Cell::new(std::ptr::null_mut()), std::cell::Cell::new(std::ptr::null_mut())]
    };
}
#[cfg(target_os = "motor")]
#[unsafe(no_mangle)]
extern "C" fn wasmtime_tls_get(slot: usize) -> *mut u8 {
    TLS.with(|slots| slots[slot].get())
}
#[cfg(target_os = "motor")]
#[unsafe(no_mangle)]
extern "C" fn wasmtime_tls_set(slot: usize, pointer: *mut u8) {
    TLS.with(|slots| slots[slot].set(pointer));
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--help") {
        println!("compile TARGET core|component INPUT OUTPUT");
        return Ok(());
    }
    #[cfg(target_os = "motor")]
    {
        let caps = moto_sys::ProcessStaticPage::get().capabilities;
        ensure!(
            moto_sys::caps::ProcessRole::from_caps(caps) == moto_sys::caps::ProcessRole::None,
            "requires role None"
        );
        ensure!(
            caps & !moto_sys::caps::CAP_FS_WRITE == 0,
            "excess capabilities"
        );
        println!("role=None caps={caps:#x}");
    }
    ensure!(
        args.len() == 5,
        "compile TARGET core|component INPUT OUTPUT"
    );
    ensure!(
        matches!(args[1].as_str(), "x86_64-unknown-motor" | "pulley64"),
        "unsupported qualification target"
    );
    ensure!(
        std::fs::metadata(&args[3])?.len() <= 64 << 20,
        "qualification input exceeds 64 MiB"
    );
    let mut config = Config::new();
    config.target(&args[1])?.motor_runtime();
    let engine = Engine::new(&config)?;
    let input = std::fs::read(&args[3])?;
    let output = match args[2].as_str() {
        "core" => engine.precompile_module(&input)?,
        "component" => engine.precompile_component(&input)?,
        _ => wasmtime::bail!("expected core or component"),
    };
    let path = args.get(4).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    std::io::Write::write_all(&mut file, &output)?;
    println!(
        "compile PASS target={} kind={} input={} output={}",
        args[1],
        args[2],
        input.len(),
        output.len()
    );
    Ok(())
}
