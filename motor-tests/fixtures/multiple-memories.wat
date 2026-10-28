(module
  (memory $first 1)
  (memory $second 1)
  (data (memory $first) (i32.const 4) "A")
  (data (memory $second) (i32.const 8) "B")
  (func (export "_start")
    i32.const 4 i32.load8_u $first i32.const 65 i32.ne if unreachable end
    i32.const 8 i32.load8_u $second i32.const 66 i32.ne if unreachable end
    i32.const 8 i32.load8_u $first if unreachable end
    i32.const 4 i32.load8_u $second if unreachable end))
