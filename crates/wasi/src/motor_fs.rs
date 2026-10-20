//! Motor directory capabilities are absolute paths below a preopen: every
//! operation composes the handle's path with the guest's relative path and
//! calls the native path API. A handle names a place, not an object: a
//! renamed directory is not followed, and a replacement under the same name
//! is reached. Open files are native descriptors and keep their object.
use std::{io, sync::Arc};

fn native_error(e: moto_rt::Error) -> io::Error {
    use moto_rt::Error::*;
    let kind = match e {
        NotFound => io::ErrorKind::NotFound,
        NotAllowed => io::ErrorKind::PermissionDenied,
        AlreadyInUse => io::ErrorKind::AlreadyExists,
        NotADirectory => io::ErrorKind::NotADirectory,
        NotImplemented => io::ErrorKind::Unsupported,
        InvalidArgument | InvalidFilename => io::ErrorKind::InvalidInput,
        OutOfMemory => io::ErrorKind::OutOfMemory,
        StorageFull => io::ErrorKind::StorageFull,
        FileTooLarge => io::ErrorKind::FileTooLarge,
        BadHandle => io::ErrorKind::NotFound,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, e.to_string())
}
fn unsupported<T>() -> io::Result<T> {
    Err(io::ErrorKind::Unsupported.into())
}
pub fn ambient_authority() {}
pub mod time {
    #[derive(Clone, Copy)]
    pub struct SystemTime(std::time::SystemTime);
    impl SystemTime {
        pub fn from_std(t: std::time::SystemTime) -> Self {
            Self(t)
        }
        pub fn into_std(self) -> std::time::SystemTime {
            self.0
        }
    }
}
pub mod fs {
    use super::*;
    use moto_rt::fs as rt;
    use std::{
        ffi::OsString,
        path::{Component, Path, PathBuf},
        sync::{Mutex, PoisonError},
        time::{Duration, UNIX_EPOCH},
    };
    // A native descriptor and the lock serializing positioned I/O on it: the
    // native reads and writes use the descriptor's own position.
    struct Open {
        fd: moto_rt::RtFd,
        position: Mutex<()>,
    }
    impl Open {
        fn at<T>(
            &self,
            offset: u64,
            io: impl FnOnce(moto_rt::RtFd) -> moto_rt::Result<T>,
        ) -> io::Result<T> {
            let offset = i64::try_from(offset).map_err(|_| io::ErrorKind::InvalidInput)?;
            let _position = self.position.lock().unwrap_or_else(PoisonError::into_inner);
            rt::seek(self.fd, offset, rt::SEEK_SET)
                .and_then(|_| io(self.fd))
                .map_err(native_error)
        }
    }
    impl Drop for Open {
        fn drop(&mut self) {
            let _ = rt::close(self.fd);
        }
    }
    #[derive(Clone)]
    enum Object {
        Dir(Arc<str>),
        File(Arc<Open>),
    }
    #[derive(Clone)]
    pub struct File {
        object: Object,
        read: bool,
        write: bool,
    }
    impl File {
        fn attr(&self) -> io::Result<rt::FileAttr> {
            match &self.object {
                Object::Dir(path) => rt::stat(path),
                Object::File(open) => rt::get_file_attr(open.fd),
            }
            .map_err(native_error)
        }
        /// The identity of the object the handle reaches now.
        pub(crate) fn identity(&self) -> io::Result<u128> {
            Ok(self.attr()?.entry_id)
        }
        pub fn metadata(&self) -> io::Result<Metadata> {
            Metadata::from_file(self)
        }
        pub fn sync_data(&self) -> io::Result<()> {
            match &self.object {
                Object::Dir(_) => Ok(()),
                Object::File(open) => rt::fsync(open.fd).map_err(native_error),
            }
        }
        pub fn sync_all(&self) -> io::Result<()> {
            self.sync_data()
        }
        pub fn set_len(&self, n: u64) -> io::Result<()> {
            let Object::File(open) = &self.object else {
                return Err(io::ErrorKind::IsADirectory.into());
            };
            // Like POSIX ftruncate, a file not open for writing is an invalid argument.
            if !self.write {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            rt::truncate(open.fd, n).map_err(native_error)
        }
        pub fn set_times(&self, _: std::fs::FileTimes) -> io::Result<()> {
            unsupported()
        }
        pub(crate) fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
            if !self.read {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            let Object::File(open) = &self.object else {
                return Err(io::ErrorKind::IsADirectory.into());
            };
            open.at(offset, |fd| rt::read(fd, buf))
        }
        pub(crate) fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize> {
            let Object::File(open) = &self.object else {
                return Err(io::ErrorKind::IsADirectory.into());
            };
            if !self.write {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            open.at(offset, |fd| rt::write(fd, buf))
        }
        pub fn try_clone(&self) -> io::Result<Self> {
            Ok(self.clone())
        }
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct FileType(u8);
    impl FileType {
        pub fn is_dir(self) -> bool {
            self.0 == rt::FILETYPE_DIRECTORY
        }
        pub fn is_file(self) -> bool {
            self.0 == rt::FILETYPE_FILE
        }
        pub fn is_symlink(self) -> bool {
            false
        }
    }
    pub struct Metadata(rt::FileAttr);
    impl Metadata {
        pub fn from_file(f: &File) -> io::Result<Self> {
            f.attr().map(Self)
        }
        pub(crate) fn identity(&self) -> u128 {
            self.0.entry_id
        }
        pub fn file_type(&self) -> FileType {
            FileType(self.0.file_type)
        }
        pub fn is_dir(&self) -> bool {
            self.file_type().is_dir()
        }
        pub fn len(&self) -> u64 {
            self.0.size
        }
        pub fn accessed(&self) -> io::Result<super::time::SystemTime> {
            timestamp(self.0.accessed)
        }
        pub fn modified(&self) -> io::Result<super::time::SystemTime> {
            timestamp(self.0.modified)
        }
        pub fn created(&self) -> io::Result<super::time::SystemTime> {
            timestamp(self.0.created)
        }
    }
    fn timestamp(ns: u128) -> io::Result<super::time::SystemTime> {
        let secs = u64::try_from(ns / 1_000_000_000).map_err(|_| io::ErrorKind::InvalidData)?;
        let t = UNIX_EPOCH
            .checked_add(Duration::new(secs, (ns % 1_000_000_000) as u32))
            .ok_or(io::ErrorKind::InvalidData)?;
        Ok(super::time::SystemTime::from_std(t))
    }
    #[derive(Clone, Copy)]
    pub enum FollowSymlinks {
        Yes,
        No,
    }
    pub enum SystemTimeSpec {
        Absolute(super::time::SystemTime),
        SymbolicNow,
    }
    pub struct DirOptions;
    impl DirOptions {
        pub fn new() -> Self {
            Self
        }
    }
    #[derive(Default)]
    pub struct OpenOptions {
        read: bool,
        write: bool,
        create: bool,
        exclusive: bool,
        truncate: bool,
    }
    impl OpenOptions {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn read(&mut self, v: bool) -> &mut Self {
            self.read = v;
            self
        }
        pub fn write(&mut self, v: bool) -> &mut Self {
            self.write = v;
            self
        }
        pub fn create(&mut self, v: bool) -> &mut Self {
            self.create = v;
            self
        }
        pub fn create_new(&mut self, v: bool) -> &mut Self {
            self.exclusive = v;
            self.create |= v;
            self
        }
        pub fn truncate(&mut self, v: bool) -> &mut Self {
            self.truncate = v;
            self
        }
        pub fn follow(&mut self, _: FollowSymlinks) -> &mut Self {
            self
        }
    }
    // `Path::components` drops a trailing `/` or `/.`, which names a directory.
    fn names_dir(path: &Path) -> bool {
        path.to_str()
            .is_some_and(|p| p.ends_with('/') || p.ends_with("/."))
    }
    // Validate the capability boundary before I/O: no absolute paths, NULs,
    // overlong names or `..` above the handle.
    fn names(path: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        let mut depth = 0usize;
        for c in path.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or(io::ErrorKind::PermissionDenied)?;
                    names.push("..".into());
                }
                Component::Normal(n) => {
                    let n = n.to_str().ok_or(io::ErrorKind::InvalidInput)?;
                    if n.contains('\0') || n.len() > rt::MAX_FILENAME_LEN {
                        return Err(io::ErrorKind::InvalidInput.into());
                    }
                    names.push(n.to_owned());
                    depth += 1;
                }
                _ => return Err(io::ErrorKind::PermissionDenied.into()),
            }
        }
        Ok(names)
    }
    fn join(full: &mut String, name: &str) {
        if !full.ends_with('/') {
            full.push('/');
        }
        full.push_str(name);
    }
    fn kind(path: &str) -> io::Result<FileType> {
        rt::stat(path)
            .map(|attr| FileType(attr.file_type))
            .map_err(native_error)
    }
    // The absolute path below `start` that `path` names. Leaving a component
    // with `..` requires it to be a directory, as walking it would.
    fn resolve(start: &File, path: &Path) -> io::Result<String> {
        let Object::Dir(base) = &start.object else {
            return Err(io::ErrorKind::NotADirectory.into());
        };
        let mut full = base.to_string();
        let mut lengths = Vec::new();
        for name in names(path)? {
            if name == ".." {
                if !kind(&full)?.is_dir() {
                    return Err(io::ErrorKind::NotADirectory.into());
                }
                full.truncate(lengths.pop().ok_or(io::ErrorKind::PermissionDenied)?);
            } else {
                lengths.push(full.len());
                join(&mut full, &name);
            }
        }
        if full.len() >= rt::MAX_PATH_LEN {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(full)
    }
    fn is_base(start: &File, path: &str) -> bool {
        matches!(&start.object, Object::Dir(base) if &**base == path)
    }
    // An existing object; a trailing `/` or `/.` requires a directory.
    fn locate(start: &File, path: &Path) -> io::Result<(String, rt::FileAttr)> {
        let full = resolve(start, path)?;
        let attr = rt::stat(&full).map_err(native_error)?;
        if names_dir(path) && attr.file_type != rt::FILETYPE_DIRECTORY {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        Ok((full, attr))
    }
    pub fn open_ambient_dir(path: &Path, _: ()) -> io::Result<File> {
        let path = std::path::absolute(path)?;
        let mut full = String::from("/");
        for c in path.components() {
            match c {
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => full.truncate(full.rfind('/').unwrap_or(0).max(1)),
                Component::Normal(n) => {
                    join(&mut full, n.to_str().ok_or(io::ErrorKind::InvalidInput)?)
                }
                Component::Prefix(_) => return Err(io::ErrorKind::InvalidInput.into()),
            }
        }
        if full.len() >= rt::MAX_PATH_LEN {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if !kind(&full)?.is_dir() {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        Ok(File {
            object: Object::Dir(full.into()),
            read: true,
            write: true,
        })
    }
    pub fn open(start: &File, path: &Path, opts: &OpenOptions) -> io::Result<File> {
        let dir = names_dir(path);
        if opts.create && dir {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        let full = resolve(start, path)?;
        let mut flags = 0;
        if opts.read {
            flags |= rt::O_READ;
        }
        if opts.write {
            flags |= rt::O_WRITE;
        }
        if opts.truncate {
            // Like POSIX ftruncate, truncating what is not open for writing is invalid.
            if !opts.write {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            flags |= rt::O_TRUNCATE;
        }
        let fd = if opts.create {
            flags |= if opts.exclusive {
                rt::O_CREATE_NEW
            } else {
                rt::O_CREATE
            };
            match rt::open(&full, flags) {
                Ok(fd) => fd,
                // The native open reports an existing directory as an invalid argument.
                Err(moto_rt::Error::InvalidArgument) if kind(&full)?.is_dir() => {
                    return Err(io::ErrorKind::IsADirectory.into());
                }
                Err(e) => return Err(native_error(e)),
            }
        } else {
            let attr = rt::stat(&full).map_err(native_error)?;
            if attr.file_type == rt::FILETYPE_DIRECTORY {
                // As on POSIX, a directory cannot be opened for writing.
                if opts.write || opts.truncate {
                    return Err(io::ErrorKind::IsADirectory.into());
                }
                return Ok(File {
                    object: Object::Dir(full.into()),
                    read: opts.read,
                    write: opts.write,
                });
            }
            if dir {
                return Err(io::ErrorKind::NotADirectory.into());
            }
            rt::open(&full, flags).map_err(native_error)?
        };
        Ok(File {
            object: Object::File(Arc::new(Open {
                fd,
                position: Mutex::new(()),
            })),
            read: opts.read,
            write: opts.write,
        })
    }
    pub fn stat(start: &File, path: &Path, _: FollowSymlinks) -> io::Result<Metadata> {
        locate(start, path).map(|(_, attr)| Metadata(attr))
    }
    pub fn create_dir(start: &File, path: &Path, _: &DirOptions) -> io::Result<()> {
        rt::mkdir(&resolve(start, path)?).map_err(native_error)
    }
    fn remove(start: &File, path: &Path, directory: bool) -> io::Result<()> {
        let (full, attr) = locate(start, path)?;
        if is_base(start, &full) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if (attr.file_type == rt::FILETYPE_DIRECTORY) != directory {
            return Err(if directory {
                io::ErrorKind::NotADirectory
            } else {
                io::ErrorKind::IsADirectory
            }
            .into());
        }
        if directory {
            rt::rmdir(&full)
        } else {
            rt::unlink(&full)
        }
        .map_err(|e| not_empty(native_error(e), directory))
    }
    // moto_rt::Error has no "directory not empty"; sys-io sends FileTooLarge.
    fn not_empty(e: io::Error, directory: bool) -> io::Error {
        if directory && e.kind() == io::ErrorKind::FileTooLarge {
            io::ErrorKind::DirectoryNotEmpty.into()
        } else {
            e
        }
    }
    pub fn remove_dir(start: &File, path: &Path) -> io::Result<()> {
        remove(start, path, true)
    }
    pub fn remove_file(start: &File, path: &Path) -> io::Result<()> {
        remove(start, path, false)
    }
    pub fn rename(start: &File, path: &Path, to: &File, dest: &Path) -> io::Result<()> {
        let (source, attr) = locate(start, path)?;
        if is_base(start, &source) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let directory = attr.file_type == rt::FILETYPE_DIRECTORY;
        if !directory && names_dir(dest) {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let target = resolve(to, dest)?;
        if is_base(to, &target) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        // Motor replaces a destination of the other kind; POSIX refuses.
        match rt::stat(&target) {
            Ok(t) => match (directory, t.file_type == rt::FILETYPE_DIRECTORY) {
                (false, true) => return Err(io::ErrorKind::IsADirectory.into()),
                (true, false) => return Err(io::ErrorKind::NotADirectory.into()),
                _ => {}
            },
            Err(moto_rt::Error::NotFound) => {}
            Err(e) => return Err(native_error(e)),
        }
        rt::rename(&source, &target).map_err(|e| not_empty(native_error(e), directory))
    }
    pub struct DirEntry {
        attr: rt::FileAttr,
        name: OsString,
    }
    impl DirEntry {
        pub fn metadata(&self) -> io::Result<Metadata> {
            Ok(Metadata(self.attr))
        }
        pub fn file_name(&self) -> OsString {
            self.name.clone()
        }
    }
    pub fn read_base_dir(start: &File) -> io::Result<std::vec::IntoIter<io::Result<DirEntry>>> {
        let Object::Dir(path) = &start.object else {
            return Err(io::ErrorKind::NotADirectory.into());
        };
        let fd = rt::opendir(path).map_err(native_error)?;
        let mut entries = Vec::new();
        let result = loop {
            match rt::readdir(fd) {
                Ok(Some(entry)) => {
                    let name = std::str::from_utf8(&entry.fname[..entry.fname_size as usize])
                        .map_err(|_| io::ErrorKind::InvalidData)?;
                    entries.push(Ok(DirEntry {
                        attr: entry.attr,
                        name: name.into(),
                    }));
                }
                Ok(None) => break Ok(()),
                Err(e) => break Err(native_error(e)),
            }
        };
        let _ = rt::closedir(fd);
        result?;
        Ok(entries.into_iter())
    }
    pub fn read_link(_: &File, _: &Path) -> io::Result<PathBuf> {
        unsupported()
    }
    pub fn symlink(_: &Path, _: &File, _: &Path) -> io::Result<()> {
        unsupported()
    }
    pub fn hard_link(_: &File, _: &Path, _: &File, _: &Path) -> io::Result<()> {
        unsupported()
    }
    pub fn set_times(
        _: &File,
        _: &Path,
        _: Option<SystemTimeSpec>,
        _: Option<SystemTimeSpec>,
    ) -> io::Result<()> {
        unsupported()
    }
    pub fn set_times_nofollow(
        d: &File,
        p: &Path,
        a: Option<SystemTimeSpec>,
        m: Option<SystemTimeSpec>,
    ) -> io::Result<()> {
        set_times(d, p, a, m)
    }
}
