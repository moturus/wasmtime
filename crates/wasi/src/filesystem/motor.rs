use super::{
    Advice, DescriptorFlags, DescriptorStat, DescriptorType, MetadataHashValue, PlatformFile,
};
use crate::motor_fs::fs::{self, FileType, FollowSymlinks, Metadata, OpenOptions};
pub(crate) use fs::{remove_file as remove_file_or_symlink, symlink};
use std::{io, path::Path};
pub(crate) fn get_flags(_: &PlatformFile) -> io::Result<DescriptorFlags> {
    Ok(DescriptorFlags::empty())
}
pub(crate) fn advise(_: &PlatformFile, _: u64, _: u64, _: Advice) -> io::Result<()> {
    Ok(())
}
pub(crate) fn maybe_dir(_: &mut OpenOptions) {}
pub(crate) fn descriptor_type(ft: FileType) -> DescriptorType {
    if ft.is_dir() {
        DescriptorType::Directory
    } else {
        DescriptorType::RegularFile
    }
}
pub(crate) fn write_at_cursor_unspecified(f: &PlatformFile, b: &[u8], p: u64) -> io::Result<usize> {
    f.write_at(b, p)
}
pub(crate) fn read_at_cursor_unspecified(
    f: &PlatformFile,
    b: &mut [u8],
    p: u64,
) -> io::Result<usize> {
    f.read_at(b, p)
}
pub(crate) fn append_cursor_unspecified(f: &PlatformFile, b: &[u8]) -> io::Result<usize> {
    f.write_at(b, f.metadata()?.len())
}
pub(crate) fn stat(f: &PlatformFile) -> io::Result<DescriptorStat> {
    Ok(DescriptorStat::new(&Metadata::from_file(f)?, 1))
}
pub(crate) fn stat_at(d: &PlatformFile, p: &Path, s: FollowSymlinks) -> io::Result<DescriptorStat> {
    Ok(DescriptorStat::new(&fs::stat(d, p, s)?, 1))
}
pub(crate) fn metadata_hash(f: &PlatformFile) -> io::Result<MetadataHashValue> {
    Ok(MetadataHashValue::new(f.id))
}
pub(crate) fn metadata_hash_at(
    d: &PlatformFile,
    p: &Path,
    _: FollowSymlinks,
) -> io::Result<MetadataHashValue> {
    metadata_hash(&fs::open(d, p, OpenOptions::new().read(true))?)
}
pub(crate) fn is_same_file(a: &PlatformFile, b: &PlatformFile) -> io::Result<bool> {
    Ok(a.id == b.id)
}
