[![Rust](https://img.shields.io/badge/rust-stable%202024-orange.svg)](https://www.rust-lang.org)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![CI](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml/badge.svg)](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml)

# MRVE32 (Modular RISC-V Emulator 32-bit)


**A modular, deterministic RV32IM bare-metal microcontroller & hardware accelerator emulator in Rust.**

MRVE32 is a lightweight RV32IM emulator designed for bare-metal firmware execution, custom SoC peripheral development, and hardware coprocessor prototyping. It provides a modular
memory-mapped I/O (MMIO) bus and an extension slot for domain-specific instructions, with optional Python bindings for testbenches.

---

## Key Architecture & Features

- **RV32IM + Zicsr + Zifencei**: base integer ISA, multiplication/division, CSR instructions, `fence`/`fence.i`.
- **Machine-mode privileged architecture**: precise synchronous exceptions (illegal instruction, misaligned jump target, instruction/load/store access faults, `ecall`, `ebreak`) delivered through `mtvec` with `mepc`/`mcause`/`mtval`/`mstatus` updated as hardware does; `mret`.
- **Interrupts & timer**: machine external, software and timer interrupts with spec priority (MEI > MSI > MTI), direct and vectored `mtvec`, `wfi` that stalls the hart until an interrupt is pending. A CLINT device (`msip`, `mtime`, `mtimecmp`, SiFive memory map) is included; any device can raise interrupt lines through `BusDevice::irq_lines`.
- **Counters (Zicntr)**: 64-bit `mcycle`/`minstret` with `mcountinhibit`, read-only `cycle`/`instret` shadows; `mhpmcounter*`/`mhpmevent*` are hardwired to zero.
- **Strict CSR file**: accesses to unimplemented CSRs and writes to read-only CSRs raise illegal-instruction exceptions; WARL fields (`mstatus`, `mtvec`, `mepc`, `mie`, `mip`) are masked. Implemented: `mstatus`, `mstatush`, `misa`, `mie`, `mip`, `mtvec`, `mscratch`, `mepc`, `mcause`, `mtval`, `mcountinhibit`, `mcycle[h]`, `minstret[h]`, `cycle[h]`, `instret[h]`, `mvendorid`, `marchid`, `mimpid`, `mhartid`, `mconfigptr`.
- **Tested against riscv-tests**: passes all 64 applicable tests from [`riscv-tests`](https://github.com/riscv-software-src/riscv-tests) (`rv32ui-p-*`, `rv32um-p-*` and the M-mode `rv32mi-p-*` tests). This is not the official RISC-V Architectural Certification suite (`riscv-arch-test`).
- **Bare-Metal & Deterministic**: Machine mode only, no MMU, virtual memory, or OS overhead.
- **Modular Bus Architecture**: Devices, RAM, and memory-mapped peripherals implement the `BusDevice` trait. The bus performs offset translation and address routing.
- **Hardware Coprocessor Slot**: extension point via `trait Coprocessor` for custom instructions (opcodes `0x0B` and `0x2B`). Register updates are committed atomically and `x0` stays zero.
- **Unidirectional Ownership**: Clean Rust ownership model (`cpu.step(&mut bus)`). The CPU core maintains zero references to the system bus.
- **No dependencies**: the core library depends only on Rust `std`.
- **SNN layer accelerator model (draft v0)**: memory-mapped spiking-layer accelerator (LIF/IF neurons, `i16` potentials, `i8` weights, reset by subtraction) with a bit-exact golden model behind a swappable `SnnBackend` trait, an analytical cycle model and a completion interrupt. See [`docs/snn-accelerator-v0.md`](docs/snn-accelerator-v0.md).
- **Python Testbenches (PyO3)**: optional `python` feature providing an `Emulator` class for stimulus generation and register-level verification.

### Not implemented (yet)

The `time`/`timeh` CSRs (read `mtime` from the CLINT instead), an interrupt controller (PLIC), User/Supervisor modes, PMP, debug triggers, and the C, A, F/D extensions.
Firmware must be built with `-march=rv32im_zicsr_zifencei -mabi=ilp32`. Misaligned loads and stores are performed in hardware rather than trapped.

---

## Architecture Overview

```text
        +------------------------------------+
        |         CPU Core (RV32IM)          |
        |  - 32x Regs (x0 hardwired to zero) |<--- [ Coprocessor Slot ]
        |  - Program Counter (PC)            |     (Custom opcodes 0x0B, 0x2B)
        +-----------------+------------------+
                            | cpu.step(&mut bus)
        +-----------------v------------------+
        |             System Bus             |
        +--------+------------------+--------+
                |                  |
        +----------v---------+  +-----v--------------+
        |        RAM         |  | CLINT, Custom      |
        | (Little-Endian MM) |  | Peripherals, IRQs  |
        +--------------------+  +--------------------+
```
---

## Rust Usage

Add MRVE32 to your `Cargo.toml`, then use the core:

```rust
use mrve32::{Bus, BusDevice, Cpu, Ram};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bus = Bus::new();
    let mut ram = Ram::new(1024);

    // Write machine instructions into RAM:
    // addi x1, x0, 42  (0x02A0_0093)
    ram.write_u32(0, 0x02A0_0093)?;
    bus.map_device(0x1000, 1024, Box::new(ram))?;

    let mut cpu = Cpu::new(0x1000);

    // `step` returns `Some(trap)` if the instruction raised an exception.
    // The trap has already been taken: `pc` points to the `mtvec` handler.
    if let Some(trap) = cpu.step(&mut bus) {
        return Err(trap.into());
    }

    assert_eq!(cpu.read_reg(1), 42);
    assert_eq!(cpu.pc, 0x1004);

    Ok(())
}
```

### Time and interrupts

Each `step` is one clock cycle: the CPU samples `mip` from the bus
interrupt lines, takes an enabled pending interrupt (or executes one
instruction), then calls `BusDevice::tick(1)` on every mapped device.
Map a `Clint` to get `mtime`/`mtimecmp` and software interrupts:

```rust
use mrve32::devices::clint::CLINT_SIZE;
use mrve32::{Bus, Clint};

let mut bus = Bus::new();
// mtime advances once every 10 CPU cycles
bus.map_device(0x0200_0000, CLINT_SIZE, Box::new(Clint::with_divider(10)))
    .unwrap();
```

Custom peripherals raise interrupts by overriding `BusDevice::irq_lines`
(return a mask of `MIP_MEIP`/`MIP_MSIP`/`MIP_MTIP`) and advance their own
state in `BusDevice::tick`.

---
## Python Testbenches (PyO3)

Build the Python extension module:

```bash
cargo build --features python && cp target/debug/libmrve32.so ./mrve32.so
```

Script bare-metal execution from Python:

```python
import mrve32

# Initialize core, RAM mapped at 0x1000 and a CLINT at 0x0200_0000
# (pass clint_base=None to omit it)
emu = mrve32.Emulator(pc=0x1000, ram_size=4096)

# Load raw RV32I machine words:
# addi x1, x0, 15
# addi x2, x0, 27
# add  x3, x1, x2
emu.load_program(0x1000, [0x00F00093, 0x01B00113, 0x002081B3])

for _ in range(3):
    trap = emu.step()  # None, or a Trap(cause, is_interrupt, tval, epc, description)
    assert trap is None

assert emu.read_reg(3) == 42
assert emu.pc == 0x100C
```


---
## Verification & Quality Assurance

Run the full test suite (unit tests, README examples, an interrupt-driven firmware test and the riscv-tests suite):

```bash
cargo test
```

Run only the riscv-tests suite (per-test results are printed):

```bash
cargo test --test compliance -- --nocapture
```

The test ELF files in `tests/isa/` are built reproducibly from source with a
pinned `riscv-tests` revision (requires a RISC-V GCC cross toolchain):

```bash
scripts/build-riscv-tests.sh
```

Run static analysis and linter checks:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```
---
## License

Licensed under Apache License, Version 2.0.
