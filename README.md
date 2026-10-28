[![Rust](https://img.shields.io/badge/rust-stable%202024-orange.svg)](https://www.rust-lang.org)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![CI](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml/badge.svg)](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml)

# MRVE32 (Modular RISC-V Emulator 32-bit)


**A modular, deterministic RV32I bare-metal microcontroller & hardware accelerator emulator in Rust.**

MRVE32 is a lightweight, zero-dependency RV32I emulator designed for bare-metal firmware execution, custom SoC peripheral development, and hardware coprocessor prototyping. It provides a modular
memory-mapped I/O (MMIO) bus and an extension slot for domain-specific instructions, with optional Python bindings for hardware-in-the-loop testbenches.

---

## Key Architecture & Features

- **Strict RV32I Integer Base ISA**: Fully verified implementations of R, I, S, B, U, and J instruction formats.
- **Bare-Metal & Deterministic**: Runs in Machine-level bare-metal mode with no MMU, virtual memory, or OS overhead.
- **Modular Bus Architecture**: Devices, RAM, and memory-mapped peripherals implement the `BusDevice` trait. The bus performs transparent offset translation and address routing.
- **Hardware Coprocessor Slot**: Dedicated extension point via `trait Coprocessor` for custom instructions (reserved opcodes `0x0B` and `0x2B`), with automatic hardware `x0` invariant enforcement.
- **Unidirectional Ownership**: Clean Rust ownership model (`cpu.step(&mut bus)`). The CPU core maintains zero references to the system bus.
- **Zero Core Dependencies**: Core library depends only on Rust `std` (ready for embedded environments).
- **Python Verification Testbenches (PyO3)**: Optional `python` feature providing an `Emulator` class for automated stimulus generation and register-level verification.
- **Rigorous Verification**: 54 comprehensive unit and integration tests covering sign extensions, arithmetic wrapping, memory boundaries, and control flow edge cases.

---

## Architecture Overview

```text
        +------------------------------------+
        |          CPU Core (RV32I)          |
        |  - 32x Regs (x0 hardwired to zero) |<--- [ Coprocessor Slot ]
        |  - Program Counter (PC)            |     (Custom opcodes 0x0B, 0x2B)
        +-----------------+------------------+
                            | cpu.step(&mut bus)
        +-----------------v------------------+
        |             System Bus             |
        +--------+------------------+--------+
                |                  |
        +----------v---------+  +-----v--------------+
        |        RAM         |  | Custom Peripherals |
        | (Little-Endian MM) |  |  (UART, Timers, IP)|
        +--------------------+  +--------------------+
──────
## Rust Usage

Add MRVE32 to your Cargo.toml:

use mrve32::{Bus, Cpu, Ram};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bus = Bus::new();
    let mut ram = Ram::new(1024);

    // Write machine instructions into RAM:
    // addi x1, x0, 42  (0x02A0_0093)
    ram.write_u32(0, 0x02A0_0093)?;
    bus.map_device(0x1000, 1024, Box::new(ram));

    let mut cpu = Cpu::new(0x1000);
    cpu.step(&mut bus)?;

    assert_eq!(cpu.read_reg(1), 42);
    assert_eq!(cpu.pc, 0x1004);

    Ok(())
}
──────
## Python Testbenches (PyO3)

Build the Python extension module:

cargo build --features python && cp target/debug/libmrve32.so ./mrve32.so

Script bare-metal execution from Python:

import mrve32

# Initialize core and RAM mapped at 0x1000
emu = mrve32.Emulator(pc=0x1000, ram_size=4096)

# Load raw RV32I machine words:
# addi x1, x0, 15
# addi x2, x0, 27
# add  x3, x1, x2
emu.load_program(0x1000, [0x00F00093, 0x01B00113, 0x002081B3])

for _ in range(3):
    emu.step()

assert emu.read_reg(3) == 42
assert emu.pc == 0x100C
──────
## Verification & Quality Assurance

Run the test suite:
cargo test
Run static analysis and linter checks:

cargo clippy --all-targets --all-features -- -D warnings
──────
## License

Licensed under Apache License, Version 2.0.
