SECTIONS {
  .motor_wasm 0x10000000 : { KEEP(*(.motor_wasm)) }
}
INSERT AFTER .bss;
