;; Calls the stack check's async host function from a guarded fiber.
(module
  (import "host" "yield" (func $yield))
  (func (export "run") (call $yield)))
