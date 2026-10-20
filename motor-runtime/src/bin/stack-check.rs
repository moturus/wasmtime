#[cfg(target_os = "motor")]
fn main() -> wasmtime::Result<()> {
    use std::{
        ops::Range,
        sync::{Arc, Mutex},
    };
    use wasmtime::{Config, Engine, Linker, Module, StackCreator, StackMemory, Store, ensure};
    use wasmtime_cli::motor_stack::StackPool;
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
    fn metric(id: u32) -> u64 {
        let mut metrics = [moto_sys::stats::MetricEntry::default(); 128];
        let (n, total) =
            moto_sys::SysRay::query_stats(moto_sys::current_pid(), &mut metrics).unwrap();
        assert_eq!(n, total);
        metrics[..n].iter().find(|e| e.metric == id).unwrap().value
    }
    // This process's reserved bytes, and the RAM backing its mappings.
    fn charge() -> u64 {
        metric(0)
    }
    fn resident() -> u64 {
        metric(79)
    }
    const PAGE: usize = 4096;
    /// # Safety
    /// `address` must be inside a leased stack.
    unsafe fn touch(address: usize) {
        unsafe { (address as *mut u8).write_volatile(123) }
    }
    let pool = Arc::new(StackPool::default());
    // The reservation is the stack and its two guards; nothing is committed
    // until touched, and its lowest page is usable.
    let before = charge();
    let largest = pool.new_stack(2 << 20, false)?;
    let range = largest.range();
    let guard = largest.guard_range();
    ensure!(
        charge() - before == (2 << 20) + 2 * PAGE as u64,
        "stack reservation"
    );
    ensure!(
        range.len() == 2 << 20
            && guard.end as usize == range.start
            && guard.end as usize - guard.start as usize == PAGE,
        "stack shape"
    );
    let before = resident();
    unsafe {
        touch(range.start);
        touch(range.end - 1);
    }
    let committed = resident() - before;
    ensure!(
        committed == 2 * PAGE as u64,
        "two touches commit {committed} bytes"
    );
    println!("stack committed bytes={committed} size=2097152 touched=2");
    drop(largest);
    // A cached stack keeps its bytes; a zeroed request gets a fresh one.
    let stack = pool.new_stack(64 << 10, false)?;
    let address = stack.range().start;
    unsafe { touch(address) };
    drop(stack);
    let stack = pool.new_stack(64 << 10, false)?;
    ensure!(
        stack.range().start == address && unsafe { (address as *const u8).read_volatile() } == 123,
        "pooled reuse"
    );
    let zeroed = pool.new_stack(64 << 10, true)?;
    let fresh = zeroed.range().start;
    ensure!(
        fresh != address && unsafe { (fresh as *const u8).read_volatile() } == 0,
        "zeroed stack"
    );
    drop((stack, zeroed));
    let before = charge();
    for _ in 0..2048 {
        let pool = StackPool::default();
        drop(pool.new_stack(64 << 10, true)?);
        drop(pool);
        ensure!(charge() == before, "stack teardown leaked");
    }
    // As many stacks as serve's default request limit, each with its top
    // page touched, commit a page each plus the leases' own heap. Afterwards
    // only that heap may stay reserved, which is less than one stack.
    drop(pool);
    let pool = Arc::new(StackPool::default());
    let reservation = (2 << 20) + 2 * PAGE as u64;
    let before = (charge(), resident());
    let mut leased = Vec::new();
    for _ in 0..1000 {
        let stack = pool.new_stack(2 << 20, false)?;
        unsafe { touch(stack.range().end - 1) };
        leased.push(stack);
    }
    let reserved = charge() - before.0;
    let committed = resident() - before.1;
    let pages = 1000 * PAGE as u64;
    ensure!(
        reserved >= 1000 * reservation && (pages..pages + (1 << 20)).contains(&committed),
        "1000 leased stacks reserve {reserved} and commit {committed} bytes"
    );
    drop(leased);
    drop(pool);
    let left = charge().saturating_sub(before.0);
    ensure!(
        left < reservation,
        "leased stacks left {left} bytes reserved"
    );
    println!("leased stacks=1000 committed={committed} left={left}");
    // Wasmtime's stacks, recorded so the callback can check where it runs.
    struct Recorder(StackPool, Mutex<Vec<Range<usize>>>);
    unsafe impl StackCreator for Recorder {
        fn new_stack(&self, size: usize, zeroed: bool) -> wasmtime::Result<Box<dyn StackMemory>> {
            let stack = self.0.new_stack(size, zeroed)?;
            self.1.lock().unwrap().push(stack.range());
            Ok(stack)
        }
    }
    let recorder = Arc::new(Recorder(StackPool::default(), Mutex::new(Vec::new())));
    let contains = {
        let recorder = recorder.clone();
        move |address: usize| {
            let ranges = recorder.1.lock().unwrap();
            ranges.iter().any(|range| range.contains(&address))
        }
    };
    let mut config = Config::new();
    config
        .target("pulley64")?
        .motor_runtime()
        .with_host_stack(recorder);
    let engine = Engine::new(&config)?;
    let module = unsafe { Module::deserialize_file(&engine, &args[2])? };
    let mut linker = Linker::new(&engine);
    let overflow = mode == "overflow";
    linker.func_wrap_async("host", "yield", move |_caller, (): ()| {
        let contains = contains.clone();
        Box::new(async move {
            #[inline(never)]
            fn check_frame(contains: &dyn Fn(usize) -> bool) -> wasmtime::Result<()> {
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
            check_frame(&contains)?;
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
            check_frame(&contains)?;
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
    println!("guarded fibers PASS teardown=2048 yield=2048 leased=1000 lazy=true");
    Ok(())
}
#[cfg(not(target_os = "motor"))]
fn main() {}
