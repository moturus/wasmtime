(module
  (memory (export "memory") 1)
  (data (i32.const 0) "abc")
  (data (i32.const 1) "Z")
  (data "P")
  (func $init
    i32.const 1 i32.load8_u i32.const 90 i32.ne
    if unreachable end
    i32.const 16 i32.const 1 i32.store8)
  (start $init)
  (func (export "_start")
    i32.const 0 i32.load8_u i32.const 97 i32.ne if unreachable end
    i32.const 2 i32.load8_u i32.const 99 i32.ne if unreachable end
    i32.const 3 i32.load8_u if unreachable end
    i32.const 16 i32.load8_u i32.const 1 i32.ne if unreachable end
    i32.const 1 memory.grow i32.const 1 i32.ne if unreachable end
    i32.const 65536 i32.const 0 i32.const 1 memory.init 2
    data.drop 2
    i32.const 65536 i32.load8_u i32.const 80 i32.ne if unreachable end
    i32.const 65537 i32.load8_u if unreachable end))
