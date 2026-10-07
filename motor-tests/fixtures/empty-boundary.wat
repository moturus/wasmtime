(module
  (memory 1)
  (data (i32.const 65536) "")
  (func (export "_start")
    i32.const 65535 i32.load8_u if unreachable end))
