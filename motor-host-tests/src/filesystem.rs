use std::{io, path::Path};
use wasmtime_wasi::motor_fs::fs::{self, DirOptions, FollowSymlinks, OpenOptions};

fn usage() -> u64 {
    let mut metrics = [moto_sys::stats::MetricEntry::default(); 128];
    let (written, total) =
        moto_sys::SysRay::query_stats(moto_sys::current_pid(), &mut metrics).unwrap();
    assert_eq!(written, total);
    metrics[..written]
        .iter()
        .find(|m| m.metric == 0)
        .unwrap()
        .value
}

pub(super) fn check() -> io::Result<()> {
    let base = format!("/user/tmp/wasm-baseline-fs-{}", std::process::id());
    std::fs::create_dir(&base)?;
    let original = format!("{base}/grant");
    let moved = format!("{base}/moved");
    std::fs::create_dir(&original)?;
    std::fs::write(format!("{original}/leaf"), b"original")?;
    let granted = fs::open_ambient_dir(Path::new(&original), ())?;
    let held = fs::open(&granted, Path::new("leaf"), OpenOptions::new().read(true))?;
    assert_eq!(held.metadata()?.len(), 8);
    std::fs::rename(&original, &moved)?;
    std::fs::create_dir(&original)?;
    std::fs::write(format!("{original}/leaf"), b"replacement")?;
    assert_eq!(
        fs::stat(&granted, Path::new("leaf"), FollowSymlinks::Yes)?.len(),
        8
    );
    assert_eq!(held.metadata()?.len(), 8);
    assert_eq!(std::fs::metadata(format!("{original}/leaf"))?.len(), 11);
    assert_eq!(
        fs::open(
            &granted,
            Path::new("../grant/leaf"),
            OpenOptions::new().read(true)
        )
        .err()
        .unwrap()
        .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        fs::open(
            &granted,
            Path::new("/user/tmp"),
            OpenOptions::new().read(true)
        )
        .err()
        .unwrap()
        .kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        fs::open(
            &granted,
            Path::new("leaf/../leaf"),
            OpenOptions::new().read(true)
        )
        .err()
        .unwrap()
        .kind(),
        io::ErrorKind::NotADirectory
    );
    for missing in ["missing", "missing/leaf"] {
        let error = fs::open(&granted, Path::new(missing), OpenOptions::new().read(true));
        assert_eq!(error.err().unwrap().kind(), io::ErrorKind::NotFound);
    }
    // A trailing `/` or `/.` names a directory.
    for file in ["leaf/", "leaf//", "leaf/."] {
        let error = fs::open(&granted, Path::new(file), OpenOptions::new().read(true));
        assert_eq!(error.err().unwrap().kind(), io::ErrorKind::NotADirectory);
    }
    let error = fs::open(
        &granted,
        Path::new("fresh/"),
        OpenOptions::new().write(true).create(true),
    );
    assert_eq!(error.err().unwrap().kind(), io::ErrorKind::IsADirectory);
    let error = fs::remove_file(&granted, Path::new("leaf/"));
    assert_eq!(error.unwrap_err().kind(), io::ErrorKind::NotADirectory);
    let error = fs::rename(&granted, Path::new("leaf"), &granted, Path::new("other/"));
    assert_eq!(error.unwrap_err().kind(), io::ErrorKind::NotADirectory);
    assert_eq!(
        held.set_len(0).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    fs::create_dir(&granted, Path::new("nested"), &DirOptions::new())?;
    for dir in ["nested", "nested/", "."] {
        let error = fs::open(
            &granted,
            Path::new(dir),
            OpenOptions::new().read(true).write(true),
        );
        assert_eq!(error.err().unwrap().kind(), io::ErrorKind::IsADirectory);
        fs::open(&granted, Path::new(dir), OpenOptions::new().read(true))?;
    }
    let created = fs::open(
        &granted,
        Path::new("nested/new"),
        OpenOptions::new().read(true).write(true).create_new(true),
    )?;
    created.set_len(32)?;
    assert_eq!(created.metadata()?.len(), 32);
    fs::rename(
        &granted,
        Path::new("nested/new"),
        &granted,
        Path::new("renamed"),
    )?;
    assert_eq!(created.metadata()?.len(), 32);
    assert_eq!(fs::read_base_dir(&granted)?.count(), 3);
    created.sync_all()?;
    fs::remove_file(&granted, Path::new("renamed"))?;
    fs::open(
        &granted,
        Path::new("nested/kept"),
        OpenOptions::new().write(true).create(true),
    )?;
    let error = fs::remove_dir(&granted, Path::new("nested"));
    assert_eq!(error.unwrap_err().kind(), io::ErrorKind::DirectoryNotEmpty);
    fs::create_dir(&granted, Path::new("empty"), &DirOptions::new())?;
    let error = fs::rename(&granted, Path::new("empty"), &granted, Path::new("nested"));
    assert_eq!(error.unwrap_err().kind(), io::ErrorKind::DirectoryNotEmpty);
    fs::remove_dir(&granted, Path::new("empty"))?;
    fs::remove_file(&granted, Path::new("nested/kept"))?;
    fs::remove_dir(&granted, Path::new("nested"))?;
    println!(
        "PASS FS parent-ID confinement, rename/replacement, rights, create, resize, rename, enumerate, flush, remove"
    );
    // Warm the cached native client and allocator before testing repeated calls.
    for _ in 0..32 {
        assert_eq!(
            fs::stat(&granted, Path::new("leaf"), FollowSymlinks::Yes)?.len(),
            8
        );
    }
    let before = usage();
    for _ in 0..1024 {
        assert_eq!(
            fs::stat(&granted, Path::new("leaf"), FollowSymlinks::Yes)?.len(),
            8
        );
        assert_eq!(usage(), before, "filesystem client/lookup leaked");
    }
    println!("PASS FS client reuse cycles=1024 charge={before}");
    std::thread::spawn(move || -> io::Result<()> {
        assert_eq!(held.metadata()?.len(), 8);
        assert_eq!(
            fs::stat(&granted, Path::new("leaf"), FollowSymlinks::Yes)?.len(),
            8
        );
        Ok(())
    })
    .join()
    .unwrap()?;
    std::fs::remove_dir_all(&base)?;
    assert!(!Path::new(&base).exists());
    println!("PASS FS worker-thread ownership and fixture cleanup");
    Ok(())
}
