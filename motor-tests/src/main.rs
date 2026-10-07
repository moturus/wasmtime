use object::{Object, ObjectSection};
use wasmtime::{Config, Engine, ExternType, Global, Instance, Memory, Module, Result, Store, Val};

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

fn engine(target: &str, coalesce: bool) -> Result<Engine> {
    let mut config = Config::new();
    config.target(target)?;
    if target == "pulley64" {
        config.motor_runtime();
    }
    config.memory_init_static(coalesce);
    let engine = Engine::new(&config)?;
    assert!(!engine.get_memory_init_cow());
    assert_eq!(engine.get_memory_guaranteed_dense_image_size(), 0);
    Ok(engine)
}

fn text_size(bytes: &[u8]) -> u64 {
    object::File::parse(bytes)
        .unwrap()
        .section_by_name(".text")
        .unwrap()
        .size()
}

fn compatibility_hash(engine: &Engine) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    engine.precompile_compatibility_hash().hash(&mut hash);
    hash.finish()
}

fn run(engine: &Engine, source: &str) -> Result<()> {
    let module = Module::new(engine, source)?;
    let mut store = Store::new(engine, ());
    let imports = module
        .imports()
        .map(|import| match import.ty() {
            ExternType::Memory(ty) => Ok(Memory::new(&mut store, ty)?.into()),
            ExternType::Global(ty) => Ok(Global::new(&mut store, ty, Val::I32(3))?.into()),
            _ => panic!("unexpected fixture import"),
        })
        .collect::<Result<Vec<_>>>()?;
    let instance = Instance::new(&mut store, &module, &imports)?;
    instance
        .get_typed_func::<(), ()>(&mut store, "_start")?
        .call(&mut store, ())
}

fn dense_segments() -> String {
    let mut source = String::from("(module (memory 1)");
    for i in 0..4096 {
        source.push_str(&format!("(data (i32.const {}) \"abcdefgh\")", i * 8));
    }
    source.push_str("(func (export \"_start\") i32.const 32767 i32.load8_u i32.const 104 i32.ne if unreachable end))");
    source
}

fn main() -> Result<()> {
    #[cfg(target_os = "motor")]
    {
        let caps = moto_sys::ProcessStaticPage::get().capabilities;
        assert_eq!(
            moto_sys::caps::ProcessRole::from_caps(caps),
            moto_sys::caps::ProcessRole::None
        );
        assert_eq!(caps & !moto_sys::caps::CAP_FS_WRITE, 0);
        println!("role=None caps={caps:#x}");
    }
    let dense = dense_segments();
    let mut reassigned = Config::new();
    reassigned.target("x86_64-unknown-motor")?;
    let native = Engine::new(&reassigned)?;
    assert!(!native.get_memory_init_cow());
    assert_eq!(native.get_memory_guaranteed_dense_image_size(), 0);
    assert!(text_size(&native.precompile_module(dense.as_bytes())?) <= 4096);
    reassigned.target("x86_64-unknown-linux-gnu")?;
    let linux = Engine::new(&reassigned)?;
    assert!(linux.get_memory_init_cow());
    assert_eq!(linux.get_memory_guaranteed_dense_image_size(), 16 << 20);
    println!("resolved target defaults PASS");
    let sparse = "(module (memory 33) (data (i32.const 0) \"A\") (data (i32.const 2097152) \"B\") (func (export \"_start\")))";
    for target in ["x86_64-unknown-motor", "pulley64"] {
        let baseline = engine(target, false)?;
        let fixed = engine(target, true)?;
        let original = baseline.precompile_module(dense.as_bytes())?;
        let coalesced = fixed.precompile_module(dense.as_bytes())?;
        assert!(text_size(&coalesced) < text_size(&original) / 8);
        assert!(fixed.precompile_module(sparse.as_bytes())?.len() < 64 * 1024);
        println!(
            "{target} dense PASS text={} -> {} sparse PASS",
            text_size(&original),
            text_size(&coalesced)
        );
        let component = format!(
            "(component {} (core instance $i (instantiate $m)) (func (export \"run\") (canon lift (core func $i \"_start\"))))",
            dense.replacen("(module", "(core module $m", 1)
        );
        let compiled = fixed.precompile_component(component.as_bytes())?;
        let uncoalesced = baseline.precompile_component(component.as_bytes())?;
        assert!(text_size(&compiled) < text_size(&uncoalesced) / 8);
        if target == "pulley64" {
            let component = wasmtime::component::Component::new(&fixed, &component)?;
            let mut store = Store::new(&fixed, ());
            let instance =
                wasmtime::component::Linker::new(&fixed).instantiate(&mut store, &component)?;
            let function = instance.get_typed_func::<(), ()>(&mut store, "run")?;
            function.call(&mut store, ())?;
        }
        println!(
            "{target} component PASS text={} -> {}",
            text_size(&uncoalesced),
            text_size(&compiled)
        );
    }
    let original = engine("pulley64", false)?;
    let fixed = engine("pulley64", true)?;
    assert_ne!(compatibility_hash(&original), compatibility_hash(&fixed));
    let mut sparse_config = Config::new();
    sparse_config
        .target("pulley64")?
        .motor_runtime()
        .memory_guaranteed_dense_image_size(16 << 20);
    let sparse_engine = Engine::new(&sparse_config)?;
    assert_ne!(
        compatibility_hash(&fixed),
        compatibility_hash(&sparse_engine)
    );
    assert!(sparse_engine.precompile_module(sparse.as_bytes())?.len() > 2 << 20);
    println!("compiler policy cache separation PASS");
    let fixtures = [
        (
            "imported-memory",
            "(module (import \"\" \"m\" (memory 1)) (data (i32.const 0) \"A\") (func (export \"_start\") i32.const 0 i32.load8_u i32.const 65 i32.ne if unreachable end))",
            false,
        ),
        (
            "imported-global",
            "(module (import \"\" \"offset\" (global i32)) (memory 1) (data (global.get 0) \"A\") (func (export \"_start\") i32.const 3 i32.load8_u i32.const 65 i32.ne if unreachable end))",
            false,
        ),
        (
            "overlap-passive-start",
            include_str!("../fixtures/overlap-passive-start.wat"),
            false,
        ),
        (
            "multiple-memory",
            include_str!("../fixtures/multiple-memories.wat"),
            false,
        ),
        (
            "empty-boundary",
            include_str!("../fixtures/empty-boundary.wat"),
            false,
        ),
        (
            "sparse",
            include_str!("../fixtures/sparse-fallback.wat"),
            false,
        ),
        ("oob-data", include_str!("../fixtures/oob-data.wat"), true),
        ("oob-empty", include_str!("../fixtures/oob-empty.wat"), true),
        (
            "active-dropped",
            include_str!("../fixtures/active-dropped.wat"),
            true,
        ),
    ];
    for (name, source, trap) in fixtures {
        for engine in [&original, &fixed] {
            match run(engine, source) {
                Ok(()) => assert!(!trap, "{name} lost its bounds trap"),
                Err(error) => {
                    assert!(trap, "{name}: {error:?}");
                    assert!(
                        format!("{error:?}").contains("out of bounds memory access"),
                        "{name}: {error:?}"
                    );
                }
            }
        }
        println!("{name} PASS");
    }
    run(&fixed, &dense)?;
    let path = std::env::temp_dir().join(format!("motor-policy-{}.cwasm", std::process::id()));
    let bytes = fixed.precompile_module(dense.as_bytes())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    drop(file);
    // SAFETY: this dedicated test VM owns the freshly compiled artifact and
    // keeps its bytes unchanged through the module's entire lifetime.
    let module = unsafe { Module::deserialize_file(&fixed, &path) }?;
    let mut store = Store::new(&fixed, ());
    Instance::new(&mut store, &module, &[])?
        .get_typed_func::<(), ()>(&mut store, "_start")?
        .call(&mut store, ())?;
    drop(module);
    std::fs::remove_file(path)?;
    println!("allocator-backed file loading PASS");
    println!("Motor compiler policy PASS");
    Ok(())
}
