//! Path semantics beyond rename of the preopen: NULs, renamed and deleted
//! directory handles, replacement after stat, unsupported operations and the
//! cost of the adapter against native path lookups.
use std::{io, path::Path, time::Instant};
use wasmtime_wasi::motor_fs::fs::{self, DirOptions, FollowSymlinks, OpenOptions};

fn read() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    options
}

pub(super) fn check() -> io::Result<()> {
    let base = format!("/none/tmp/wasm-interleavings-{}", std::process::id());
    std::fs::create_dir(&base)?;
    let granted = fs::open_ambient_dir(Path::new(&base), ())?;

    let nul = fs::open(&granted, Path::new("bad\0name"), &read());
    assert_eq!(nul.err().unwrap().kind(), io::ErrorKind::InvalidInput);

    // A subdirectory handle names its path, so it does not follow a rename.
    fs::create_dir(&granted, Path::new("sub"), &DirOptions::new())?;
    std::fs::write(format!("{base}/sub/x"), b"in sub")?;
    let sub = fs::open(&granted, Path::new("sub"), &read())?;
    assert_eq!(
        fs::stat(&sub, Path::new("x"), FollowSymlinks::Yes)?.len(),
        6
    );
    std::fs::rename(format!("{base}/sub"), format!("{base}/sub2"))?;
    let renamed = fs::stat(&sub, Path::new("x"), FollowSymlinks::Yes)
        .err()
        .expect("handle followed the renamed directory");
    assert_eq!(renamed.kind(), io::ErrorKind::NotFound);

    // A handle to a deleted directory reaches a same-named replacement.
    fs::create_dir(&granted, Path::new("gone"), &DirOptions::new())?;
    let gone = fs::open(&granted, Path::new("gone"), &read())?;
    std::fs::remove_dir(format!("{base}/gone"))?;
    std::fs::create_dir(format!("{base}/gone"))?;
    std::fs::write(format!("{base}/gone/new"), b"replacement")?;
    assert_eq!(
        fs::stat(&gone, Path::new("new"), FollowSymlinks::Yes)?.len(),
        11
    );

    // Every operation resolves the path again: nothing survives a
    // replacement between a stat and the next use.
    std::fs::write(format!("{base}/flip"), b"file")?;
    assert!(!fs::stat(&granted, Path::new("flip"), FollowSymlinks::Yes)?.is_dir());
    std::fs::remove_file(format!("{base}/flip"))?;
    std::fs::create_dir(format!("{base}/flip"))?;
    assert!(fs::stat(&granted, Path::new("flip"), FollowSymlinks::Yes)?.is_dir());

    let unsupported = [
        fs::symlink(Path::new("x"), &granted, Path::new("link")),
        fs::hard_link(&granted, Path::new("flip"), &granted, Path::new("hard")),
        fs::read_link(&granted, Path::new("flip")).map(drop),
    ];
    for result in unsupported {
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
    }
    println!(
        "PASS FS NUL rejection, renamed directory handle ({renamed}), replaced directory handle, stat/replace, unsupported link operations"
    );

    // Cost of a three-level adapter stat against native path stat.
    std::fs::create_dir_all(format!("{base}/a/b"))?;
    std::fs::write(format!("{base}/a/b/c"), b"deep")?;
    let native = format!("{base}/a/b/c");
    let rounds = 512;
    let started = Instant::now();
    for _ in 0..rounds {
        assert_eq!(
            fs::stat(&granted, Path::new("a/b/c"), FollowSymlinks::Yes)?.len(),
            4
        );
    }
    let adapter = started.elapsed() / rounds;
    let started = Instant::now();
    for _ in 0..rounds {
        assert_eq!(std::fs::metadata(&native)?.len(), 4);
    }
    let path = started.elapsed() / rounds;
    std::fs::remove_dir_all(&base)?;
    println!(
        "PASS FS lookup cost adapter_us={} native_stat_us={}",
        adapter.as_micros(),
        path.as_micros()
    );
    Ok(())
}
