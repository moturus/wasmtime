//! Owned executable code on Motor's separate RX and RW mappings.
use crate::prelude::*;
use moto_sys::{SysHandle, SysMem, sys_mem::PAGE_SIZE_SMALL};

pub(super) struct CodeMapping {
    address: u64,
    len: usize,
}

impl CodeMapping {
    pub(super) fn new(code: &[u8]) -> Result<Self> {
        assert!(!code.is_empty());
        let (executable, writable) = SysMem::map2(
            SysHandle::SELF,
            SysMem::F_SHARE_SELF | SysMem::F_READABLE | SysMem::F_EXECUTABLE,
            u64::MAX,
            u64::MAX,
            PAGE_SIZE_SMALL,
            code.len().div_ceil(PAGE_SIZE_SMALL as usize) as u64,
        )
        .map_err(|err| format_err!("allocate executable code: {err}"))?;
        let executable = Self {
            address: executable,
            len: code.len(),
        };
        let writable = Self {
            address: writable,
            len: code.len(),
        };
        // The fresh mappings share frames and have enough space for the
        // complete text section. Neither is exposed while it is being filled.
        unsafe {
            core::ptr::copy_nonoverlapping(code.as_ptr(), writable.address as *mut u8, code.len());
        }
        // Unmap revokes this alias on all CPUs before returning. Motor is
        // x86-64, where the instruction and data caches are coherent.
        drop(writable);
        Ok(executable)
    }

    pub(super) fn as_slice(&self) -> &[u8] {
        // The mapping is readable, immutable after publication, and owned
        // until the containing CodeMemory is no longer used.
        unsafe { core::slice::from_raw_parts(self.address as *const u8, self.len) }
    }
}

impl Drop for CodeMapping {
    fn drop(&mut self) {
        SysMem::free(self.address).expect("release code mapping");
    }
}
