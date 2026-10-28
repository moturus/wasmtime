(module
  (memory 257)
  (data (i32.const 0) "A")
  (data (i32.const 16777216) "B")
  (func (export "_start")
    i32.const 0 i32.load8_u i32.const 65 i32.ne if unreachable end
    i32.const 16777216 i32.load8_u i32.const 66 i32.ne if unreachable end
    i32.const 8388608 i32.load8_u if unreachable end))
