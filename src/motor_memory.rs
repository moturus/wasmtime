//! Owned, nonmoving Motor reservations for Wasm linear memories.
//!
//! Each memory lazily reserves its whole bounded capacity once, so growth never
//! copies and untouched pages read as zero. Reservations are sparse: only
//! touched pages use RAM, and as on Linux there is no process-wide total.
use moto_sys::{SysHandle, SysMem, sys_mem::PAGE_SIZE_SMALL};
use wasmtime::{LinearMemory, MemoryCreator, MemoryType, Result, bail};

/// Largest capacity of one linear memory: a 32-bit memory's whole range. A
/// 64-bit memory fails to grow past it instead of moving.
pub const MAX_MEMORY_BYTES: usize = 4 << 30;

/// A [`MemoryCreator`] that backs each memory with one lazy reservation.
#[derive(Debug)]
pub struct Reservations {
    per_memory: usize,
}

impl Default for Reservations {
    fn default() -> Self {
        Self::new(MAX_MEMORY_BYTES)
    }
}

impl Reservations {
    /// Limits the capacity of each memory.
    pub fn new(per_memory: usize) -> Self {
        Self { per_memory }
    }
}

struct Reservation {
    address: u64,
    size: usize,
    capacity: usize,
}

// The mapping is exclusive to this memory, stays at one address for its
// lifetime and is released on drop.
unsafe impl LinearMemory for Reservation {
    fn byte_size(&self) -> usize {
        self.size
    }
    fn byte_capacity(&self) -> usize {
        self.capacity
    }
    fn grow_to(&mut self, new_size: usize) -> Result<()> {
        if new_size > self.capacity {
            bail!("linear memory is limited to {} bytes", self.capacity);
        }
        self.size = new_size;
        Ok(())
    }
    fn as_ptr(&self) -> *mut u8 {
        self.address as *mut u8
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        SysMem::unmap(SysHandle::SELF, 0, u64::MAX, self.address)
            .expect("release Wasm memory reservation");
    }
}

unsafe impl MemoryCreator for Reservations {
    fn new_memory(
        &self,
        ty: MemoryType,
        minimum: usize,
        maximum: Option<usize>,
        _reserved_size_in_bytes: Option<usize>,
        guard_size_in_bytes: usize,
    ) -> Result<Box<dyn LinearMemory>, String> {
        // Code checks bounds explicitly, and lazy pages cannot act as guards.
        if guard_size_in_bytes != 0 || ty.is_shared() {
            return Err("Motor memories have no guard region and are not shared".into());
        }
        let capacity = maximum.unwrap_or(self.per_memory).min(self.per_memory);
        if minimum > capacity {
            return Err(format!(
                "initial memory of {minimum} bytes exceeds the {capacity}-byte limit"
            ));
        }
        let pages = capacity.max(1).div_ceil(PAGE_SIZE_SMALL as usize) as u64;
        let address = SysMem::map(
            SysHandle::SELF,
            SysMem::F_READABLE | SysMem::F_WRITABLE | SysMem::F_LAZY,
            u64::MAX,
            u64::MAX,
            PAGE_SIZE_SMALL,
            pages,
        )
        .map_err(|e| format!("Wasm memory reservation failed: {e:?}"))?;
        Ok(Box::new(Reservation {
            address,
            size: minimum,
            capacity,
        }))
    }
}
