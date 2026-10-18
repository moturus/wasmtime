//! Qualifies owned Motor memory reservations as None. Arguments are the
//! host-precompiled Pulley `memory-grow` and `memory-too-large` fixtures.
#[cfg(target_os = "motor")]
fn main() -> wasmtime::Result<()> {
    use moto_sys::stats::MemoryStats;
    use std::sync::Arc;
    use wasmtime::{Config, Engine, Instance, MemoryCreator, MemoryType, Module, Store, ensure};
    use wasmtime_cli::motor_memory::{MAX_MEMORY_BYTES, Reservations};
    wasmtime_cli::motor::check_authority()?;
    let args: Vec<_> = std::env::args().collect();
    ensure!(args.len() == 3, "memory-check GROW.cwasm TOO-LARGE.cwasm");
    fn charge() -> u64 {
        let mut metrics = [moto_sys::stats::MetricEntry::default(); 128];
        let (n, total) =
            moto_sys::SysRay::query_stats(moto_sys::current_pid(), &mut metrics).unwrap();
        assert_eq!(n, total);
        metrics[..n].iter().find(|e| e.metric == 0).unwrap().value
    }
    fn used() -> u64 {
        MemoryStats::get().unwrap().used()
    }
    const PAGE: usize = 64 << 10;
    let new = |creator: &Reservations, ty: MemoryType, guard: usize| {
        let bytes = |pages: u64| usize::try_from(pages).unwrap() * PAGE;
        let (minimum, maximum) = (bytes(ty.minimum()), ty.maximum().map(bytes));
        creator.new_memory(ty, minimum, maximum, Some(0), guard)
    };

    // The per-memory limit, the guest maximum and refusals.
    let creator = Reservations::new(4 << 20);
    let unbounded = new(&creator, MemoryType::new(1, None), 0).map_err(wasmtime::Error::msg)?;
    ensure!(unbounded.byte_capacity() == 4 << 20, "per-memory capacity");
    ensure!(
        new(&creator, MemoryType::new(65, None), 0).is_err(),
        "initial size above the limit"
    );
    ensure!(
        new(&creator, MemoryType::new(1, None), PAGE).is_err(),
        "guard region accepted"
    );
    let shared = creator.new_memory(MemoryType::shared(1, 1), PAGE, Some(PAGE), Some(0), 0);
    ensure!(shared.is_err(), "shared memory accepted");
    let bounded = new(&creator, MemoryType::new(1, Some(2)), 0).map_err(wasmtime::Error::msg)?;
    ensure!(bounded.byte_capacity() == 2 * PAGE, "guest maximum ignored");
    drop((unbounded, bounded));

    // By default 4 GiB, also for 64-bit memories, with no total or count:
    // untouched reservations use almost no RAM.
    let creator = Arc::new(Reservations::default());
    let wide = new(&creator, MemoryType::new64(1, None), 0).map_err(wasmtime::Error::msg)?;
    ensure!(wide.byte_capacity() == MAX_MEMORY_BYTES, "64-bit capacity");
    ensure!(
        new(&creator, MemoryType::new64(65537, None), 0).is_err(),
        "64-bit initial size above the limit"
    );
    let before = used();
    let many = (0..64)
        .map(|_| new(&creator, MemoryType::new(1, None), 0))
        .collect::<Result<Vec<_>, _>>()
        .map_err(wasmtime::Error::msg)?;
    let held = used().saturating_sub(before);
    ensure!(
        held < 4 << 20,
        "64 untouched reservations hold {held} bytes"
    );
    drop((wide, many));

    // Through the engine: grow in 1 MiB steps to the limit, reading a zero
    // and writing one page per step; growth itself commits nothing.
    let mut config = Config::new();
    config
        .target("pulley64")?
        .motor_runtime()
        .with_host_memory(creator.clone());
    let engine = Engine::new(&config)?;
    let grow = unsafe { Module::deserialize_file(&engine, &args[1])? };
    let too_large = unsafe { Module::deserialize_file(&engine, &args[2])? };
    let started = std::time::Instant::now();
    let before = used();
    let touched;
    {
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &grow, &[])?;
        let grow_by = instance.get_typed_func::<i32, i32>(&mut store, "grow")?;
        let load = instance.get_typed_func::<i32, i32>(&mut store, "load")?;
        let write = instance.get_typed_func::<(i32, i32), ()>(&mut store, "store")?;
        let limit = (MAX_MEMORY_BYTES / PAGE) as i32;
        let mut pages = 1;
        while pages < limit {
            let step = (limit - pages).min(16);
            ensure!(
                grow_by.call(&mut store, step)? == pages,
                "growth failed at {pages} pages"
            );
            // Offsets above 2 GiB are negative as i32 but unsigned to Wasm.
            let offset = (pages as u32 * PAGE as u32) as i32;
            ensure!(load.call(&mut store, offset)? == 0, "nonzero grown page");
            write.call(&mut store, (offset, 0x5a))?;
            pages += step;
        }
        ensure!(
            grow_by.call(&mut store, 1)? == -1,
            "growth beyond the limit"
        );
        touched = used().saturating_sub(before);
        ensure!(touched < 64 << 20, "4 GiB growth used {touched} bytes");
    }
    let elapsed = started.elapsed();

    // Failed instantiation and repeated teardown leave nothing behind.
    let before = charge();
    for _ in 0..256 {
        let mut store = Store::new(&engine, ());
        ensure!(
            Instance::new(&mut store, &too_large, &[]).is_err(),
            "oversized memory instantiated"
        );
        let instance = Instance::new(&mut store, &grow, &[])?;
        let grow_by = instance.get_typed_func::<i32, i32>(&mut store, "grow")?;
        let write = instance.get_typed_func::<(i32, i32), ()>(&mut store, "store")?;
        ensure!(grow_by.call(&mut store, 127)? == 1, "teardown growth");
        for offset in (0..128 * PAGE as i32).step_by(4096) {
            write.call(&mut store, (offset, 1))?;
        }
    }
    ensure!(charge() == before, "teardown leaked memory");
    println!(
        "memory reservations PASS limit={MAX_MEMORY_BYTES} held64={held} grow_ms={} touched={touched} teardown=256",
        elapsed.as_millis()
    );
    Ok(())
}
#[cfg(not(target_os = "motor"))]
fn main() {}
