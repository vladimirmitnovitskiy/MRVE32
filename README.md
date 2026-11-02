[![Rust](https://img.shields.io/badge/rust-stable%202024-orange.svg)](https://www.rust-lang.org)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![CI](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml/badge.svg)](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml)

# MRVE32 (Modular RISC-V Emulator 32-bit)


**A modular, deterministic RV32IM bare-metal microcontroller emulator in Rust, with pluggable devices.**

MRVE32 is a lightweight RV32IM emulator designed for bare-metal firmware execution, custom SoC peripheral development, and hardware coprocessor prototyping. The core is a CPU, a memory-mapped I/O (MMIO) bus, RAM, a CLINT and an ELF loader; every other peripheral is a **device module** that anyone can write — in Rust, in Python, or as a separately built native package through a small, versioned C ABI. Python bindings make it a testbench engine.

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
- **Pluggable devices**: a device is anything implementing `BusDevice` (Rust), a Python object with `read`/`write` methods, or a native module built separately against the device C ABI ([`include/mrve32_device.h`](include/mrve32_device.h), `mrve32::plugin`). Devices get `tick()` time and can raise interrupt lines. See [`docs/devices.md`](docs/devices.md).
- **Python package (PyO3)**: `pip install`-able `mrve32` module with an `Emulator` class (ELF loading, symbols, breakpoints, run control, memory/register/CSR access, `map_device`). Typed (`py.typed` stubs).

### Device modules

| Module | Description |
|---|---|
| [`mrve32-snn`](modules/snn) | SNN layer accelerator model (draft v0): LIF/IF neurons, `i16` potentials, `i8` weights, a bit-exact golden model behind a swappable `SnnBackend`, an analytical cycle model, a completion interrupt, and a NumPy reference. [Spec](modules/snn/docs/snn-accelerator-v0.md). |

Modules live in `modules/` for now but depend on the core only through its public API and could be moved to their own repositories unchanged.

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
state in `BusDevice::tick`; see [`docs/devices.md`](docs/devices.md).

---
## Python Testbenches (PyO3)

Install the `mrve32` package (builds the extension with [maturin](https://www.maturin.rs/);
needs a Rust toolchain and Python >= 3.10):

```bash
pip install .                 # or, for development: pip install -e ".[test]"
```

Run firmware and inspect the machine:

```python
import mrve32

# RAM at 0x8000_0000 (64 KiB) and a CLINT at 0x0200_0000 by default
# (clint_base=None omits it).
emu = mrve32.Emulator(ram_size=64 * 1024)

entry = emu.load_elf("firmware.elf")      # path or bytes; sets pc to the entry
emu.add_breakpoint(emu.symbol("main"))

res = emu.run(1_000_000, stop_on_trap=True)
print(res.reason, res.steps)              # "max_steps" | "breakpoint" | "trap" | "idle"
if res.trap:
    print(hex(res.trap.cause), res.trap.description)

print(emu.regs, hex(emu.pc), emu.cycles, emu.instret)
data = emu.read_bytes(emu.symbol("buffer"), 64)  # raises mrve32.BusError if unmapped
```

Attach devices: native modules or plain Python objects ([`docs/devices.md`](docs/devices.md)):

```python
class Gpio:
    size = 4
    def __init__(self): self.pins = 0
    def read(self, offset, width): return self.pins
    def write(self, offset, width, value): self.pins = value

emu.map_device(0x1000_0000, Gpio())          # Python device
emu.map_device(0x2000_0000, mrve32.Ram(4096))  # native device
print(emu.devices)   # [(base, size, name), ...]
```

For example, with the SNN module (`pip install ./modules/snn`):

```python
import numpy as np
from mrve32 import Emulator
from mrve32_snn import SnnAccelerator, SnnDriver, reference_step

emu = Emulator()
emu.map_device(0x1000_0000, SnnAccelerator())
dev = SnnDriver(emu, 0x1000_0000)

weights = np.random.randint(-128, 128, size=(64, 32), dtype=np.int8)
dev.load_layer(weights, v_th=100, leak_shift=4)

vmem = np.zeros(32, np.int16)
for t in range(10):
    spikes = np.random.rand(64) < 0.2
    out = dev.step(spikes)                # write inputs, START, wait, read outputs
    expected, vmem = reference_step(spikes, weights, vmem, v_th=100, leak_shift=4)
    assert (out == expected).all() and (dev.read_vmem() == vmem).all()
    print(t, out.sum(), "spikes,", dev.cycles, "cycles")
```

Run the Python tests:

```bash
pytest tests/python modules/snn/tests/python
```

---
## Verification & Quality Assurance

Run the full test suite (unit tests, README examples, an interrupt-driven firmware test and the riscv-tests suite):

```bash
cargo test --workspace
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
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
---
## License

Licensed under Apache License, Version 2.0.
