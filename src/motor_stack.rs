//! Bounded, guarded host fibers using unchanged Motor mapping APIs.
use moto_sys::{SysHandle, SysMem, sys_mem::PAGE_SIZE_SMALL};
use std::{
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use wasmtime::{Result, StackCreator, StackMemory, ensure};

const PAGE: usize = PAGE_SIZE_SMALL as usize;
const MAX_SIZE: usize = 2 << 20;
const SLOTS: usize = 8;
const BASE: usize = moto_sys::CUSTOM_USERSPACE_REGION_START as usize + (16 << 20);
static OCCUPIED: AtomicU64 = AtomicU64::new(0);

struct Mapping {
    address: usize,
    alias: usize,
}
impl Mapping {
    fn new(address: usize, size: usize, writable: bool) -> Result<Self> {
        let flags = SysMem::F_SHARE_SELF
            | SysMem::F_READABLE
            | if writable { SysMem::F_WRITABLE } else { 0 };
        let (mapped, alias) = SysMem::map2(
            SysHandle::SELF,
            flags,
            u64::MAX,
            address as u64,
            PAGE_SIZE_SMALL,
            (size / PAGE) as u64,
        )
        .map_err(|e| wasmtime::format_err!("fiber mapping: {e}"))?;
        assert_eq!(mapped as usize, address);
        Ok(Self {
            address,
            alias: alias as usize,
        })
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        // Never release the address slot while either mapping remains alive.
        SysMem::free(self.address as u64).expect("unmap fiber target");
        SysMem::free(self.alias as u64).expect("unmap fiber alias");
    }
}
struct Slot(usize);
impl Slot {
    fn acquire() -> Result<Self> {
        #[allow(deprecated, reason = "maintain the upstream Rust MSRV")]
        let old = OCCUPIED
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |bits| {
                let index = bits.trailing_ones() as usize;
                (index < SLOTS).then(|| bits | (1 << index))
            })
            .map_err(|_| wasmtime::format_err!("Motor fiber stack limit reached"))?;
        Ok(Self(old.trailing_ones() as usize))
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        OCCUPIED.fetch_and(!(1 << self.0), Ordering::AcqRel);
    }
}
struct Allocation {
    guard: Mapping,
    data: Mapping,
    size: usize,
    _slot: Slot,
}
impl Allocation {
    fn new(size: usize) -> Result<Self> {
        let slot = Slot::acquire()?;
        let bottom = BASE + slot.0 * (MAX_SIZE + PAGE);
        let guard = Mapping::new(bottom, PAGE, false)?;
        let data = Mapping::new(bottom + PAGE, size, true)?;
        Ok(Self {
            guard,
            data,
            size,
            _slot: slot,
        })
    }
}

/// At most eight live mappings process-wide and two cached stacks per engine.
/// These are slice-zero qualification bounds, pending HTTP concurrency sizing.
#[derive(Default)]
pub struct StackPool(Arc<Mutex<Vec<Allocation>>>);
struct Lease {
    allocation: Option<Allocation>,
    pool: Arc<Mutex<Vec<Allocation>>>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let allocation = self.allocation.take().unwrap();
        let mut pool = self.pool.lock().unwrap();
        if pool.len() < 2 {
            pool.push(allocation);
        }
    }
}
// Each lease owns disjoint pages; only Wasmtime may access a leased stack.
unsafe impl StackCreator for StackPool {
    fn new_stack(&self, size: usize, zeroed: bool) -> Result<Box<dyn StackMemory>> {
        ensure!(
            size > 0 && size <= MAX_SIZE,
            "Motor fiber size exceeds 2 MiB"
        );
        let size = size.next_multiple_of(PAGE);
        let mut pool = self.0.lock().unwrap();
        let allocation = if let Some(index) = pool.iter().position(|a| a.size == size) {
            let allocation = pool.swap_remove(index);
            if zeroed {
                unsafe { std::ptr::write_bytes(allocation.data.address as *mut u8, 0, size) };
            }
            allocation
        } else {
            Allocation::new(size)?
        };
        Ok(Box::new(Lease {
            allocation: Some(allocation),
            pool: self.0.clone(),
        }))
    }
}
// Ranges remain mapped until the lease returns; the read-only guard catches writes.
unsafe impl StackMemory for Lease {
    fn top(&self) -> *mut u8 {
        self.range().end as *mut u8
    }
    fn range(&self) -> Range<usize> {
        let a = self.allocation.as_ref().unwrap();
        a.data.address..a.data.address + a.size
    }
    fn guard_range(&self) -> Range<*mut u8> {
        let a = self.allocation.as_ref().unwrap();
        a.guard.address as *mut u8..(a.guard.address + PAGE) as *mut u8
    }
}

/// Whether a host callback's local frame resides inside this stack arena.
pub fn contains(address: usize) -> bool {
    (BASE..BASE + SLOTS * (MAX_SIZE + PAGE)).contains(&address)
}
