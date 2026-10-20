//! Guarded, lazily committed host fibers on Motor's guarded heap maps.
use moto_sys::{SysHandle, SysMem, sys_mem::PAGE_SIZE_SMALL};
use std::{
    ops::Range,
    sync::{Arc, Mutex},
};
use wasmtime::{Result, StackCreator, StackMemory};

const PAGE: usize = PAGE_SIZE_SMALL as usize;

/// A lazy map between two pages that are never mapped: a page is committed
/// on first touch, and a touch on either guard kills the thread.
struct Allocation {
    start: usize,
    size: usize,
}
impl Allocation {
    fn new(size: usize) -> Result<Self> {
        let (start, total) = SysMem::map2(
            SysHandle::SELF,
            SysMem::F_READABLE | SysMem::F_WRITABLE | SysMem::F_LAZY | SysMem::F_GUARDED,
            u64::MAX,
            u64::MAX,
            PAGE_SIZE_SMALL,
            (size / PAGE) as u64,
        )
        .map_err(|e| wasmtime::format_err!("fiber mapping: {e}"))?;
        assert_eq!(total as usize, size + 2 * PAGE);
        Ok(Self {
            start: start as usize,
            size,
        })
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        SysMem::free(self.start as u64).expect("unmap fiber stack");
    }
}

/// Two cached stacks per engine; a cached stack keeps the pages it touched.
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
        let size = size.max(1).next_multiple_of(PAGE);
        // A cached stack may be dirty; a fresh one reads as zeros.
        let cached = if zeroed {
            None
        } else {
            let mut pool = self.0.lock().unwrap();
            pool.iter()
                .position(|a| a.size == size)
                .map(|index| pool.swap_remove(index))
        };
        let allocation = match cached {
            Some(allocation) => allocation,
            None => Allocation::new(size)?,
        };
        Ok(Box::new(Lease {
            allocation: Some(allocation),
            pool: self.0.clone(),
        }))
    }
}
// Ranges remain mapped until the lease returns; the lower guard catches overruns.
unsafe impl StackMemory for Lease {
    fn top(&self) -> *mut u8 {
        self.range().end as *mut u8
    }
    fn range(&self) -> Range<usize> {
        let a = self.allocation.as_ref().unwrap();
        a.start + PAGE..a.start + PAGE + a.size
    }
    fn guard_range(&self) -> Range<*mut u8> {
        let a = self.allocation.as_ref().unwrap();
        a.start as *mut u8..(a.start + PAGE) as *mut u8
    }
}
