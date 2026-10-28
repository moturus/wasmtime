;; 4 GiB and 64 KiB of initial 64-bit memory: valid by default; an explicit
;; 4 GiB store limit refuses it.
(module (memory (export "memory") i64 65537))
