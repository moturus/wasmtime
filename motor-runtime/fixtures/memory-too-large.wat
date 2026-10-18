;; 4 GiB and 64 KiB of initial 64-bit memory: above the per-memory limit, so
;; instantiation fails.
(module (memory (export "memory") i64 65537))
