;; A 64-bit memory grows to the 4 GiB per-memory limit and fails past it.
(module
  (memory (export "memory") i64 1)
  (func (export "grow") (param i64) (result i64) (memory.grow (local.get 0))))
