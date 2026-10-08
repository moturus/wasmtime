//! Owned, nonmoving Motor reservations for Wasm linear memories.
//!
//! Each memory lazily reserves its whole bounded capacity once, so growth never
//! copies and untouched pages read as zero. Limits bound each memory, the
//! number of memories and their aggregate reservation in the process.
use moto_sys::{SysHandle, SysMem, sys_mem::PAGE_SIZE_SMALL};
use std::sync::{Arc, Mutex};
use wasmtime::{LinearMemory, MemoryCreator, MemoryType, Result, bail};

/// Largest capacity of one linear memory.
pub const MAX_MEMORY_BYTES: usize = 96 << 20;
/// Largest total reservation of all live memories.
pub const MAX_RESERVED_BYTES: usize = 128 << 20;
/// Largest number of live memories.
pub const MAX_MEMORIES: usize = 4;

#[derive(Debug, Default)]
struct Usage {
    bytes: usize,
    memories: usize,
}

/// A [`MemoryCreator`] that backs each memory with one lazy reservation.
#[derive(Debug)]
pub struct Reservations {
    per_memory: usize,
    aggregate: usize,
    count: usize,
    usage: Arc<Mutex<Usage>>,
}

impl Default for Reservations {
    fn default() -> Self {
        Self::new(MAX_MEMORY_BYTES, MAX_RESERVED_BYTES, MAX_MEMORIES)
    }
}

impl Reservations {
    /// Limits each memory, the reservations' total and their number.
    pub fn new(per_memory: usize, aggregate: usize, count: usize) -> Self {
        Self {
            per_memory,
            aggregate,
            count,
            usage: Arc::default(),
        }
    }

    /// Reserved bytes and memories currently held.
    pub fn usage(&self) -> (usize, usize) {
        let usage = self.usage.lock().unwrap();
        (usage.bytes, usage.memories)
    }
}

struct Reservation {
    address: u64,
    size: usize,
    capacity: usize,
    mapped: usize,
    usage: Arc<Mutex<Usage>>,
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
        let mut usage = self.usage.lock().unwrap();
        usage.bytes -= self.mapped;
        usage.memories -= 1;
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
        let mapped = capacity.max(1).next_multiple_of(PAGE_SIZE_SMALL as usize);
        {
            let mut usage = self.usage.lock().unwrap();
            if usage.bytes + mapped > self.aggregate || usage.memories >= self.count {
                return Err("Wasm memory reservation limits reached".into());
            }
            usage.bytes += mapped;
            usage.memories += 1;
        }
        let address = SysMem::map(
            SysHandle::SELF,
            SysMem::F_READABLE | SysMem::F_WRITABLE | SysMem::F_LAZY,
            u64::MAX,
            u64::MAX,
            PAGE_SIZE_SMALL,
            (mapped / PAGE_SIZE_SMALL as usize) as u64,
        )
        .map_err(|e| {
            let mut usage = self.usage.lock().unwrap();
            usage.bytes -= mapped;
            usage.memories -= 1;
            format!("Wasm memory reservation failed: {e:?}")
        })?;
        Ok(Box::new(Reservation {
            address,
            size: minimum,
            capacity,
            mapped,
            usage: Arc::clone(&self.usage),
        }))
    }
}
