use std::{cell::Cell, path::PathBuf};
use wasmtime_cli::motor_elf::{ADDRESS, HEADER, MAGIC, VERSION, package};
fn put(b: &mut [u8], at: usize, value: u64, size: usize) {
    b[at..at + size].copy_from_slice(&value.to_le_bytes()[..size]);
}
fn header(b: &mut [u8], kind: u64) {
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    put(b, 16, kind, 2);
    put(b, 18, 62, 2);
    put(b, 20, 1, 4);
    put(b, 52, 64, 2);
    put(b, 58, 64, 2);
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("motor-elf-test-{}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn publication_validation_and_cleanup() {
    let fixture = Fixture::new();
    let mut base = vec![0; 8192];
    header(&mut base, 2);
    put(&mut base, 32, 64, 8);
    put(&mut base, 54, 56, 2);
    put(&mut base, 56, 1, 2);
    put(&mut base, 40, 128, 8);
    put(&mut base, 60, 4, 2);
    put(&mut base, 62, 3, 2);
    for (at, value, size) in [
        (64, 1, 4),
        (68, 5, 4),
        (72, HEADER, 8),
        (80, ADDRESS, 8),
        (96, HEADER, 8),
        (104, HEADER, 8),
        (112, HEADER, 8),
        (196, 1, 4),
        (192, 1, 4),
        (200, 6, 8),
        (208, ADDRESS, 8),
        (216, HEADER, 8),
        (224, HEADER, 8),
        (240, HEADER, 8),
    ] {
        put(&mut base, at, value, size);
    }
    for (at, value, size) in [
        (256, 13, 4),
        (260, 1, 4),
        (280, 1024, 8),
        (288, VERSION.len() as u64, 8),
        (320, 28, 4),
        (324, 3, 4),
        (344, 512, 8),
        (352, 40, 8),
    ] {
        put(&mut base, at, value, size);
    }
    base[512..552].copy_from_slice(b"\0.motor_wasm\0.motor_version\0.shstrtab\0\0\0");
    base[1024..1024 + VERSION.len()].copy_from_slice(VERSION);
    let mut code = vec![0; 1024];
    header(&mut code, 1);
    code[7] = 200;
    put(&mut code, 48, 1, 4);
    put(&mut code, 40, 64, 8);
    put(&mut code, 60, 3, 2);
    put(&mut code, 62, 1, 2);
    for (at, value, size) in [
        (128, 1, 4),
        (132, 3, 4),
        (152, 256, 8),
        (160, 32, 8),
        (192, 11, 4),
        (196, 1, 4),
        (216, 512, 8),
        (224, 16, 8),
    ] {
        put(&mut code, at, value, size);
    }
    code[256..284].copy_from_slice(b"\0.shstrtab\0.wasmtime.engine\0");
    code[512] = 0;
    code[513] = VERSION.len() as u8;
    code[514..514 + VERSION.len()].copy_from_slice(VERSION);
    let template = fixture.file("template", &base);
    let input = fixture.file("code", &code);
    let output = fixture.0.join("native");
    package(&input, &template, &output, || false).unwrap();
    let populated = std::fs::read(&output).unwrap();
    assert_eq!(&populated[8192..8208], MAGIC);
    assert_eq!(&populated[12288..], &code);
    let before = populated.clone();
    assert!(package(&input, &template, &output, || false).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), before);
    let cancelled = fixture.0.join("cancelled");
    let calls = Cell::new(0);
    assert!(
        package(&input, &template, &cancelled, || {
            calls.set(calls.get() + 1);
            calls.get() > 1
        })
        .is_err()
    );
    assert!(!cancelled.exists());
    assert!(
        !fixture
            .0
            .join(format!("cancelled.part-{}", std::process::id()))
            .exists()
    );
    for (index, offset, value) in [
        (0, 32, u64::MAX),
        (1, 80, ADDRESS + 1),
        (2, 104, 1),
        (3, 112, 3),
        (4, 216, u64::MAX),
    ] {
        let mut malformed = base.clone();
        put(&mut malformed, offset, value, 8);
        let bad = fixture.file("bad-template", &malformed);
        assert!(
            package(
                &input,
                &bad,
                &fixture.0.join(format!("bad-{index}")),
                || false
            )
            .is_err()
        );
    }
    code[514] ^= 1;
    let mismatch = fixture.file("mismatch", &code);
    assert!(
        package(&mismatch, &template, &fixture.0.join("bad-version"), || {
            false
        })
        .is_err()
    );
    base[1024] ^= 1;
    let mismatch = fixture.file("mismatch-template", &base);
    assert!(
        package(
            &input,
            &mismatch,
            &fixture.0.join("bad-template-version"),
            || false
        )
        .is_err()
    );
}
