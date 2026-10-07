//! Qualification publisher shared by core and component native artifacts.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use wasmtime::{Result, ensure};

/// Address of the OS-loaded artifact segment.
pub const ADDRESS: u64 = 0x1000_0000;
/// Size and alignment of the artifact envelope.
pub const HEADER: u64 = 4096;
/// Envelope identification bytes.
pub const MAGIC: &[u8; 16] = b"MotorWasmELF01\0\0";
/// Exact custom Wasmtime artifact version.
pub const VERSION: &[u8; 14] = b"48.0.1-motor.1";
const LIMIT: u64 = 128 << 20;

fn read<const N: usize>(f: &mut File, at: u64) -> Result<[u8; N]> {
    let mut bytes = [0; N];
    f.seek(SeekFrom::Start(at))?;
    f.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn end(start: u64, size: u64) -> Result<u64> {
    start
        .checked_add(size)
        .ok_or_else(|| wasmtime::format_err!("ELF range overflow"))
}
fn put(f: &mut File, at: u64, value: u64) -> Result<()> {
    f.seek(SeekFrom::Start(at))?;
    f.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn table(f: &mut File, h: &[u8; 64], programs: bool) -> Result<Vec<(u64, Vec<u8>)>> {
    let (at, stride, count) = if programs {
        (u64_at(h, 32), u16_at(h, 54), u16_at(h, 56))
    } else {
        (u64_at(h, 40), u16_at(h, 58), u16_at(h, 60))
    };
    ensure!(
        stride == if programs { 56 } else { 64 } && count > 0 && count <= 4096,
        "invalid ELF table shape"
    );
    ensure!(
        end(at, u64::from(stride) * u64::from(count))? <= f.metadata()?.len(),
        "ELF table out of file"
    );
    let mut entries = Vec::new();
    for i in 0..count {
        let offset = at + u64::from(i) * u64::from(stride);
        let mut bytes = vec![0; stride as usize];
        f.seek(SeekFrom::Start(offset))?;
        f.read_exact(&mut bytes)?;
        entries.push((offset, bytes));
    }
    Ok(entries)
}

fn header(f: &mut File) -> Result<[u8; 64]> {
    let h = read::<64>(f, 0)?;
    ensure!(
        &h[..7] == b"\x7fELF\x02\x01\x01" && u16_at(&h, 18) == 62 && u16_at(&h, 52) == 64,
        "expected ELF64 x86-64"
    );
    Ok(h)
}

fn sections(f: &mut File, h: &[u8; 64]) -> Result<Vec<(u64, Vec<u8>, Vec<u8>)>> {
    let entries = table(f, h, false)?;
    let index = u16_at(h, 62) as usize;
    ensure!(index < entries.len(), "invalid section names index");
    let strings = &entries[index].1;
    let count = u64_at(strings, 32);
    ensure!(count <= 65536, "section names exceed limit");
    let mut names = vec![0; count as usize];
    f.seek(SeekFrom::Start(u64_at(strings, 24)))?;
    f.read_exact(&mut names)?;
    let length = f.metadata()?.len();
    let mut result = Vec::new();
    for (at, section) in entries {
        let offset = u64_at(&section, 24);
        let size = u64_at(&section, 32);
        if u32_at(&section, 4) != 8 {
            ensure!(end(offset, size)? <= length, "section out of file");
        }
        let align = u64_at(&section, 48);
        ensure!(
            align == 0 || align.is_power_of_two(),
            "invalid section alignment"
        );
        let start = u32_at(&section, 0) as usize;
        ensure!(start < names.len(), "invalid section name");
        let finish = names[start..]
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| wasmtime::format_err!("unterminated section name"))?;
        result.push((at, section, names[start..start + finish].to_vec()));
    }
    Ok(result)
}

fn template(f: &mut File, artifact: u64) -> Result<(u64, u64)> {
    let h = header(f)?;
    ensure!(
        matches!(u16_at(&h, 16), 2 | 3),
        "expected executable template"
    );
    let mut selected = None;
    let mut ranges = Vec::new();
    for (at, p) in table(f, &h, true)? {
        if u32_at(&p, 0) != 1 {
            continue;
        }
        let offset = u64_at(&p, 8);
        let address = u64_at(&p, 16);
        let files = u64_at(&p, 32);
        let mem = u64_at(&p, 40);
        let align = u64_at(&p, 48);
        let flags = u32_at(&p, 4);
        ensure!(
            files <= mem && end(offset, files)? <= f.metadata()?.len(),
            "invalid load file range"
        );
        ensure!(
            align.is_power_of_two() && offset % align == address % align,
            "invalid load alignment"
        );
        ensure!(
            flags & !7 == 0 && flags & 3 != 3,
            "invalid or writable executable load segment"
        );
        let finish = end(address, mem)?;
        if address == ADDRESS {
            ensure!(
                selected.is_none()
                    && flags == 5
                    && files == HEADER
                    && mem == HEADER
                    && align == HEADER,
                "invalid template artifact segment"
            );
            ensure!(
                read::<4096>(f, offset)?.iter().all(|b| *b == 0),
                "template is already populated"
            );
            selected = Some((at, offset));
            ranges.push((address, end(address, end(HEADER, artifact)?)?));
        } else {
            ranges.push((address, finish));
        }
    }
    ranges.sort();
    ensure!(
        ranges.windows(2).all(|p| p[0].1 <= p[1].0),
        "overlapping ELF load segments"
    );
    let (ph, offset) = selected.ok_or_else(|| wasmtime::format_err!("missing artifact segment"))?;
    let mut section = None;
    let mut version = false;
    for (at, s, name) in sections(f, &h)? {
        if name == b".motor_version" {
            ensure!(
                !version && u64_at(&s, 32) == VERSION.len() as u64,
                "invalid template version section"
            );
            ensure!(
                &read::<{ VERSION.len() }>(f, u64_at(&s, 24))? == VERSION,
                "template version mismatch"
            );
            version = true;
        }
        if u64_at(&s, 16) == ADDRESS {
            ensure!(
                section.is_none()
                    && u64_at(&s, 24) == offset
                    && u64_at(&s, 32) == HEADER
                    && u64_at(&s, 8) == 6
                    && u64_at(&s, 48) == HEADER,
                "invalid artifact section"
            );
            section = Some(at);
        }
    }
    ensure!(version, "missing template version");
    Ok((
        ph,
        section.ok_or_else(|| wasmtime::format_err!("missing artifact section"))?,
    ))
}

fn artifact(f: &mut File) -> Result<u64> {
    let len = f.metadata()?.len();
    ensure!(len > 0 && len <= LIMIT, "native artifact exceeds limit");
    let h = header(f)?;
    ensure!(
        u16_at(&h, 16) == 1
            && h[7] == wasmtime_environ::obj::ELFOSABI_WASMTIME
            && h[8] == 0
            && matches!(
                u32_at(&h, 48),
                wasmtime_environ::obj::EF_WASMTIME_MODULE
                    | wasmtime_environ::obj::EF_WASMTIME_COMPONENT
            ),
        "expected serialized native artifact"
    );
    let mut found = false;
    for (_, s, name) in sections(f, &h)? {
        if name == b".wasmtime.engine" {
            ensure!(
                !found && u64_at(&s, 32) >= VERSION.len() as u64 + 2,
                "invalid engine section"
            );
            let prefix = read::<{ VERSION.len() + 2 }>(f, u64_at(&s, 24))?;
            ensure!(
                prefix[0] == 0 && prefix[1] as usize == VERSION.len() && &prefix[2..] == VERSION,
                "artifact version mismatch"
            );
            found = true;
        }
    }
    ensure!(found, "missing Wasmtime engine version");
    Ok(len)
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            if let Err(error) = std::fs::remove_file(&self.0) {
                eprintln!("ELF partial cleanup failed: {error}");
            }
        }
    }
}

/// Stream and atomically publish an exclusive, read-only native executable.
pub fn package(
    input: &Path,
    base: &Path,
    output: &Path,
    cancelled: impl Fn() -> bool,
) -> Result<()> {
    let mut code = File::open(input)?;
    let bytes = artifact(&mut code)?;
    let mut source = File::open(base)?;
    let (ph, sh) = template(&mut source, bytes)?;
    let length = source.metadata()?.len();
    ensure!(length <= LIMIT, "template exceeds limit");
    let offset = end(length, HEADER - 1)? & !(HEADER - 1);
    let partial = output.with_file_name(format!(
        "{}.part-{}",
        output
            .file_name()
            .ok_or_else(|| wasmtime::format_err!("output lacks filename"))?
            .to_string_lossy(),
        std::process::id()
    ));
    let mut cleanup = Cleanup(PathBuf::new());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    cleanup.0 = partial.clone();
    source.seek(SeekFrom::Start(0))?;
    code.seek(SeekFrom::Start(0))?;
    let mut buffer = [0u8; 65536];
    copy(&mut source, &mut file, length, &cancelled, &mut buffer)?;
    file.set_len(end(offset, HEADER)?)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut envelope = [0u8; 4096];
    envelope[..16].copy_from_slice(MAGIC);
    envelope[16..24].copy_from_slice(&bytes.to_le_bytes());
    envelope[24..24 + VERSION.len()].copy_from_slice(VERSION);
    file.write_all(&envelope)?;
    copy(&mut code, &mut file, bytes, &cancelled, &mut buffer)?;
    put(&mut file, ph + 8, offset)?;
    put(&mut file, ph + 32, HEADER + bytes)?;
    put(&mut file, ph + 40, HEADER + bytes)?;
    put(&mut file, sh + 24, offset)?;
    put(&mut file, sh + 32, HEADER + bytes)?;
    file.sync_all()?;
    drop(file);
    ensure!(!cancelled(), "ELF publication cancelled");
    #[cfg(target_os = "motor")]
    {
        let from = std::path::absolute(&partial)?;
        let to = std::path::absolute(output)?;
        moto_rt::fs::set_perm(
            from.to_str()
                .ok_or_else(|| wasmtime::format_err!("non-UTF8 path"))?,
            moto_rt::fs::PERM_READ | moto_rt::fs::PERM_EXEC,
        )
        .map_err(|e| wasmtime::format_err!("finalize executable permissions: {e}"))?;
        moto_rt::fs::move_noreplace(
            from.to_str().unwrap(),
            to.to_str()
                .ok_or_else(|| wasmtime::format_err!("non-UTF8 path"))?,
        )
        .map_err(|e| wasmtime::format_err!("publish executable: {e}"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o555))?;
        std::fs::hard_link(&partial, output)?;
        std::fs::remove_file(&partial)?;
    }
    cleanup.0 = PathBuf::new();
    Ok(())
}

fn copy(
    reader: &mut File,
    writer: &mut File,
    mut left: u64,
    cancelled: &impl Fn() -> bool,
    buffer: &mut [u8],
) -> Result<()> {
    while left > 0 {
        ensure!(!cancelled(), "ELF publication cancelled");
        let size = left.min(buffer.len() as u64) as usize;
        let n = reader.read(&mut buffer[..size])?;
        ensure!(n > 0, "input truncated during publication");
        writer.write_all(&buffer[..n])?;
        left -= n as u64;
    }
    Ok(())
}
