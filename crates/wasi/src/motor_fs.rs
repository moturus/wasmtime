//! Motor directory capabilities use stable entry IDs, never pathname prefixes.
use moto_io::fs::{EntryId, EntryKind, FsClient};
use std::{future::Future, io, rc::Rc};

fn call<'a, T: 'a>(
    f: impl FnOnce(Rc<FsClient>) -> std::pin::Pin<Box<dyn Future<Output = moto_rt::Result<T>> + 'a>>,
) -> io::Result<T> {
    struct State {
        runtime: moto_async::LocalRuntime,
        client: Option<Rc<FsClient>>,
    }
    thread_local! {
        static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State {
            runtime: moto_async::LocalRuntime::new(), client: None,
        });
    }
    STATE.with(|slot| {
        let mut state = slot.try_borrow_mut().map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "reentrant native filesystem call",
            )
        })?;
        let State { runtime, client } = &mut *state;
        let result = runtime.block_on(async {
            let c = match client {
                Some(c) => c.clone(),
                None => {
                    let c = FsClient::connect()?;
                    *client = Some(c.clone());
                    c
                }
            };
            f(c).await
        });
        // Do not retry a failed operation; reconnect only on a subsequent call.
        if matches!(result, Err(moto_rt::Error::NotConnected)) {
            *client = None;
        }
        result.map_err(native_error)
    })
}
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
    use std::{
        ffi::OsString,
        path::{Component, Path, PathBuf},
        time::{Duration, UNIX_EPOCH},
    };
    #[derive(Clone)]
    pub struct File {
        pub(crate) id: EntryId,
        kind: EntryKind,
        read: bool,
        write: bool,
    }
    impl File {
        pub fn metadata(&self) -> io::Result<Metadata> {
            Metadata::from_file(self)
        }
        pub fn sync_data(&self) -> io::Result<()> {
            call(|c| Box::pin(async move { c.flush().await }))
        }
        pub fn sync_all(&self) -> io::Result<()> {
            self.sync_data()
        }
        pub fn set_len(&self, n: u64) -> io::Result<()> {
            // Like POSIX ftruncate, a file not open for writing is an invalid argument.
            if self.kind != EntryKind::Directory && !self.write {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            self.check_write()?;
            call(|c| Box::pin(async move { c.resize(self.id, n).await }))
        }
        pub fn set_times(&self, _: std::fs::FileTimes) -> io::Result<()> {
            unsupported()
        }
        fn check_write(&self) -> io::Result<()> {
            if self.kind == EntryKind::Directory {
                return Err(io::ErrorKind::IsADirectory.into());
            }
            if !self.write {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            Ok(())
        }
        pub(crate) fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
            if !self.read {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            if self.kind == EntryKind::Directory {
                return Err(io::ErrorKind::IsADirectory.into());
            }
            call(|c| Box::pin(async move { c.read(self.id, offset, buf).await }))
        }
        pub(crate) fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize> {
            self.check_write()?;
            call(|c| Box::pin(async move { c.write(self.id, offset, buf).await }))
        }
        pub fn try_clone(&self) -> io::Result<Self> {
            Ok(self.clone())
        }
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct FileType(EntryKind);
    impl FileType {
        pub fn is_dir(self) -> bool {
            self.0 == EntryKind::Directory
        }
        pub fn is_file(self) -> bool {
            self.0 == EntryKind::File
        }
        pub fn is_symlink(self) -> bool {
            false
        }
    }
    pub struct Metadata(moto_io::fs::Metadata);
    impl Metadata {
        pub fn from_file(f: &File) -> io::Result<Self> {
            call(|c| Box::pin(async move { c.metadata(f.id).await })).map(Self)
        }
        pub fn file_type(&self) -> FileType {
            FileType(self.0.kind())
        }
        pub fn is_dir(&self) -> bool {
            self.file_type().is_dir()
        }
        pub fn len(&self) -> u64 {
            self.0.size
        }
        pub fn accessed(&self) -> io::Result<super::time::SystemTime> {
            timestamp(self.0.accessed.as_nanos())
        }
        pub fn modified(&self) -> io::Result<super::time::SystemTime> {
            timestamp(self.0.modified.as_nanos())
        }
        pub fn created(&self) -> io::Result<super::time::SystemTime> {
            timestamp(self.0.created.as_nanos())
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
    // Validate the capability boundary before I/O. The walk checks every
    // directory, including components followed by `..`, using entry IDs.
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
                    if n.contains('\0') {
                        return Err(io::ErrorKind::InvalidInput.into());
                    }
                    names.push(n.to_owned());
                    depth += 1;
                }
                _ => return Err(io::ErrorKind::PermissionDenied.into()),
            }
        }
        if names.iter().map(String::len).sum::<usize>() > moto_rt::fs::MAX_PATH_LEN {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(names)
    }
    async fn walk(
        _c: &Rc<FsClient>,
        start: &File,
        names: &[String],
    ) -> moto_rt::Result<(EntryId, EntryKind)> {
        let mut found = (start.id, start.kind);
        let mut stack = vec![found];
        for name in names {
            if found.1 != EntryKind::Directory {
                return Err(moto_rt::Error::NotADirectory);
            }
            if name == ".." {
                stack.pop();
                found = *stack.last().ok_or(moto_rt::Error::NotAllowed)?;
            } else {
                found = crate::motor_lookup::lookup(found.0, name).await?;
                stack.push(found);
            }
        }
        Ok(found)
    }
    pub fn open_ambient_dir(path: &Path, _: ()) -> io::Result<File> {
        let path = std::path::absolute(path)?;
        let path = path.to_str().ok_or(io::ErrorKind::InvalidInput)?;
        let (id, kind) = call(|c| Box::pin(async move { c.stat(path).await }))?;
        if kind != EntryKind::Directory {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        Ok(File {
            id,
            kind,
            read: true,
            write: true,
        })
    }
    pub fn open(start: &File, path: &Path, opts: &OpenOptions) -> io::Result<File> {
        let names = names(path)?;
        if opts.create && names_dir(path) {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        let (id, kind) = call(|c| {
            Box::pin(async move {
                if opts.create {
                    let (leaf, parents) =
                        names.split_last().ok_or(moto_rt::Error::InvalidFilename)?;
                    let (parent, kind) = walk(&c, start, parents).await?;
                    if kind != EntryKind::Directory {
                        return Err(moto_rt::Error::NotADirectory);
                    }
                    match c.create_entry(parent, EntryKind::File, leaf).await {
                        Ok(id) => Ok((id, EntryKind::File)),
                        Err(moto_rt::Error::AlreadyInUse) if !opts.exclusive => {
                            crate::motor_lookup::lookup(parent, leaf).await
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    walk(&c, start, &names).await
                }
            })
        })?;
        if kind != EntryKind::Directory && names_dir(path) {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        // As on POSIX, a directory cannot be opened for writing.
        if kind == EntryKind::Directory && (opts.write || opts.truncate) {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        let f = File {
            id,
            kind,
            read: opts.read,
            write: opts.write,
        };
        if opts.truncate {
            f.set_len(0)?;
        }
        Ok(f)
    }
    pub fn stat(start: &File, path: &Path, _: FollowSymlinks) -> io::Result<Metadata> {
        Metadata::from_file(&open(start, path, OpenOptions::new().read(true))?)
    }
    pub fn create_dir(start: &File, path: &Path, _: &DirOptions) -> io::Result<()> {
        let names = names(path)?;
        call(|c| {
            Box::pin(async move {
                let (leaf, parents) = names.split_last().ok_or(moto_rt::Error::InvalidFilename)?;
                let (parent, kind) = walk(&c, start, parents).await?;
                if kind != EntryKind::Directory {
                    return Err(moto_rt::Error::NotADirectory);
                }
                c.create_entry(parent, EntryKind::Directory, leaf)
                    .await
                    .map(|_| ())
            })
        })
    }
    fn remove(start: &File, path: &Path, kind: EntryKind) -> io::Result<()> {
        let f = open(start, path, OpenOptions::new().read(true))?;
        if f.id == start.id {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if f.kind != kind {
            return Err(if f.kind == EntryKind::Directory {
                io::ErrorKind::IsADirectory
            } else {
                io::ErrorKind::NotADirectory
            }
            .into());
        }
        call(|c| Box::pin(async move { c.delete_entry(f.id).await }))
    }
    pub fn remove_dir(start: &File, path: &Path) -> io::Result<()> {
        remove(start, path, EntryKind::Directory)
    }
    pub fn remove_file(start: &File, path: &Path) -> io::Result<()> {
        remove(start, path, EntryKind::File)
    }
    pub fn rename(start: &File, path: &Path, to: &File, dest: &Path) -> io::Result<()> {
        let source = open(start, path, OpenOptions::new().read(true))?;
        if source.id == start.id {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if source.kind != EntryKind::Directory && names_dir(dest) {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let names = names(dest)?;
        call(|c| {
            Box::pin(async move {
                let (leaf, parents) = names.split_last().ok_or(moto_rt::Error::InvalidFilename)?;
                let (parent, kind) = walk(&c, to, parents).await?;
                if kind != EntryKind::Directory {
                    return Err(moto_rt::Error::NotADirectory);
                }
                c.move_entry(source.id, parent, leaf).await
            })
        })
    }
    pub struct DirEntry {
        id: EntryId,
        name: OsString,
    }
    impl DirEntry {
        pub fn metadata(&self) -> io::Result<Metadata> {
            call(|c| Box::pin(async move { c.metadata(self.id).await })).map(Metadata)
        }
        pub fn file_name(&self) -> OsString {
            self.name.clone()
        }
    }
    pub fn read_base_dir(start: &File) -> io::Result<std::vec::IntoIter<io::Result<DirEntry>>> {
        let entries = call(|c| {
            Box::pin(async move {
                let mut entries = Vec::new();
                let mut next = c.get_first_entry(start.id).await?;
                while let Some(id) = next {
                    entries.push(DirEntry {
                        id,
                        name: c.name(id).await?.into(),
                    });
                    next = c.get_next_entry(id).await?;
                }
                Ok(entries)
            })
        })?;
        Ok(entries.into_iter().map(Ok).collect::<Vec<_>>().into_iter())
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
