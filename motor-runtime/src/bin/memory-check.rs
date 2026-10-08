//! Qualifies owned Motor memory reservations as None. Arguments are the
//! host-precompiled Pulley `memory-grow` and `memory-too-large` fixtures.
#[cfg(target_os = "motor")]
fn main() -> wasmtime::Result<()> {
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
    const PAGE: usize = 64 << 10;
    let new = |creator: &Reservations, min: u32, max: Option<u32>, guard: usize| {
        let maximum = max.map(|pages| pages as usize * PAGE);
        creator.new_memory(
            MemoryType::new(min, max),
            min as usize * PAGE,
            maximum,
            Some(0),
            guard,
        )
    };

    // Per-memory, count and aggregate limits, and release on drop.
    let creator = Reservations::new(4 << 20, 6 << 20, 2);
    let first = new(&creator, 1, None, 0).map_err(wasmtime::Error::msg)?;
    ensure!(first.byte_capacity() == 4 << 20, "per-memory capacity");
    ensure!(
        new(&creator, 65, None, 0).is_err(),
        "initial size above the limit"
    );
    ensure!(
        new(&creator, 1, None, PAGE).is_err(),
        "guard region accepted"
    );
    let shared = creator.new_memory(MemoryType::shared(1, 1), PAGE, Some(PAGE), Some(0), 0);
    ensure!(shared.is_err(), "shared memory accepted");
    let bounded = new(&creator, 1, Some(2), 0).map_err(wasmtime::Error::msg)?;
    ensure!(bounded.byte_capacity() == 2 * PAGE, "guest maximum ignored");
    ensure!(new(&creator, 1, None, 0).is_err(), "memory count limit");
    drop(bounded);
    ensure!(new(&creator, 1, None, 0).is_err(), "aggregate limit");
    drop(first);
    ensure!(creator.usage() == (0, 0), "limits did not release");

    // Through the engine: grow in 1 MiB steps to the limit, reading zeros.
    let creator = Arc::new(Reservations::default());
    let mut config = Config::new();
    config
        .target("pulley64")?
        .motor_runtime()
        .with_host_memory(creator.clone());
    let engine = Engine::new(&config)?;
    let grow = unsafe { Module::deserialize_file(&engine, &args[1])? };
    let too_large = unsafe { Module::deserialize_file(&engine, &args[2])? };
    let started = std::time::Instant::now();
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
            for offset in (pages * PAGE as i32..(pages + step) * PAGE as i32).step_by(4096) {
                ensure!(load.call(&mut store, offset)? == 0, "nonzero grown page");
                write.call(&mut store, (offset, 0x5a))?;
            }
            pages += step;
        }
        ensure!(
            grow_by.call(&mut store, 1)? == -1,
            "growth beyond the limit"
        );
        ensure!(
            creator.usage() == (MAX_MEMORY_BYTES, 1),
            "store reservation"
        );
    }
    let elapsed = started.elapsed();
    ensure!(creator.usage() == (0, 0), "store drop did not release");

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
    ensure!(creator.usage() == (0, 0), "teardown leaked a reservation");
    ensure!(charge() == before, "teardown leaked memory");
    println!(
        "memory reservations PASS limit={MAX_MEMORY_BYTES} grow_ms={} teardown=256",
        elapsed.as_millis()
    );
    Ok(())
}
#[cfg(not(target_os = "motor"))]
fn main() {}
