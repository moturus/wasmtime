#[cfg(target_os = "motor")]
fn main() -> wasmtime::Result<()> {
    use std::sync::Arc;
    use wasmtime::{Config, Engine, Linker, Module, StackCreator, Store, ensure};
    use wasmtime_cli::motor_stack::{StackPool, contains};
    wasmtime_cli::motor::check_authority()?;
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("");
    if mode == "tcp-held" {
        let mut socket = std::net::TcpStream::connect(&args[2])?;
        println!("TCP connected {}", socket.local_addr()?);
        use std::io::Read;
        socket.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        let mut bytes = Vec::new();
        match socket.read_to_end(&mut bytes) {
            Ok(_) => println!("TCP shutdown EOF"),
            // The HTTP test independently asserts server draining and exit.
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                println!("TCP shutdown reset (native zero-byte close)")
            }
            Err(e) => return Err(e.into()),
        }
        ensure!(bytes.is_empty(), "unexpected shutdown payload");
        return Ok(());
    }
    if mode == "permissions" {
        let path = &args[2];
        let attr = moto_rt::fs::stat(path).map_err(|e| wasmtime::format_err!("stat: {e}"))?;
        ensure!(
            attr.perm == moto_rt::fs::PERM_READ | moto_rt::fs::PERM_EXEC,
            "ELF mode: {}",
            attr.perm
        );
        let result = std::fs::OpenOptions::new().write(true).open(path);
        if let Ok(mut file) = result {
            use std::io::Write;
            // Native open retains an ID; permission is enforced by the write RPC.
            // Use the existing magic byte so a failed assertion cannot corrupt it.
            ensure!(
                file.write_all(&[0x7f]).is_err(),
                "published ELF remained writable"
            );
        }
        println!(
            "permissions PASS role=None caps={:#x} None=R-X",
            moto_sys::ProcessStaticPage::get().capabilities
        );
        return Ok(());
    }
    fn charge() -> u64 {
        let mut metrics = [moto_sys::stats::MetricEntry::default(); 128];
        let (n, total) =
            moto_sys::SysRay::query_stats(moto_sys::current_pid(), &mut metrics).unwrap();
        assert_eq!(n, total);
        metrics[..n].iter().find(|e| e.metric == 0).unwrap().value
    }
    let pool = Arc::new(StackPool::default());
    ensure!(
        pool.new_stack((2 << 20) + 1, true).is_err(),
        "unbounded stack size"
    );
    let stack = pool.new_stack(64 << 10, true)?;
    let address = stack.range().start;
    unsafe {
        (address as *mut u8).write_volatile(123);
    }
    drop(stack);
    let stack = pool.new_stack(64 << 10, true)?;
    ensure!(
        stack.range().start == address && unsafe { (address as *const u8).read_volatile() } == 0,
        "pooled zeroing"
    );
    drop(stack);
    let before = charge();
    for _ in 0..2048 {
        let pool = StackPool::default();
        drop(pool.new_stack(64 << 10, true)?);
        drop(pool);
        ensure!(charge() == before, "stack teardown leaked");
    }
    // A leased ninth stack must fail; returning/dropping pools releases all slots.
    drop(pool);
    let pool = Arc::new(StackPool::default());
    let mut leased = Vec::new();
    for _ in 0..8 {
        leased.push(pool.new_stack(64 << 10, true)?);
    }
    ensure!(
        pool.new_stack(64 << 10, true).is_err(),
        "unbounded stack count"
    );
    drop(leased);
    drop(pool);
    let pool = Arc::new(StackPool::default());
    let mut config = Config::new();
    config
        .target("pulley64")?
        .motor_runtime()
        .with_host_stack(pool);
    let engine = Engine::new(&config)?;
    let module = unsafe { Module::deserialize_file(&engine, &args[2])? };
    let mut linker = Linker::new(&engine);
    let overflow = mode == "overflow";
    linker.func_wrap_async("host", "yield", move |_caller, (): ()| {
        Box::new(async move {
            #[inline(never)]
            fn check_frame() -> wasmtime::Result<()> {
                let mut frame = [7_u8; 1024];
                unsafe {
                    frame.as_mut_ptr().write_volatile(7);
                }
                ensure!(
                    contains(frame.as_ptr() as usize),
                    "callback outside guarded fiber"
                );
                std::hint::black_box(frame);
                Ok(())
            }
            check_frame()?;
            if overflow {
                #[inline(never)]
                fn recurse() {
                    let mut bytes = [0_u8; 1024];
                    unsafe {
                        bytes.as_mut_ptr().write_volatile(1);
                    }
                    std::hint::black_box(recurse as fn())();
                    std::hint::black_box(bytes);
                }
                recurse();
            }
            tokio::task::yield_now().await;
            check_frame()?;
            Ok(())
        })
    })?;
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    runtime.block_on(async {
        let mut store = Store::new(&engine, ());
        let instance = linker.instantiate_async(&mut store, &module).await?;
        let func = instance.get_typed_func::<(), ()>(&mut store, "run")?;
        for _ in 0..2048 {
            func.call_async(&mut store, ()).await?;
        }
        let before_cancel = charge();
        for _ in 0..1024 {
            let mut future = Box::pin(func.call_async(&mut store, ()));
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(future.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            drop(future);
            ensure!(charge() == before_cancel, "cancelled fiber leaked");
        }
        Ok::<_, wasmtime::Error>(())
    })?;
    println!("guarded fibers PASS teardown=2048 yield=2048 limit=8 zeroing=true");
    Ok(())
}
#[cfg(not(target_os = "motor"))]
fn main() {}
