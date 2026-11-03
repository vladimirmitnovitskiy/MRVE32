[![PyPI](https://img.shields.io/pypi/v/mrve32.svg)](https://pypi.org/project/mrve32/)
[![Python](https://img.shields.io/pypi/pyversions/mrve32.svg)](https://pypi.org/project/mrve32/)
[![CI](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml/badge.svg)](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml)
[![riscv-arch-test](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/arch-test.yml/badge.svg)](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/arch-test.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-orange.svg)](https://www.rust-lang.org)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/LICENSE)

# MRVE32 (Modular RISC-V Emulator 32-bit)

**A RISC-V microcontroller you can `pip install`: run bare-metal RV32IMAC
firmware from Python or Rust, and add your own peripherals and custom
instructions in a few lines.**

```python
import mrve32

emu = mrve32.Emulator(ram_size=1 << 20)        # RV32IMAC hart, RAM, timer (CLINT)
uart = mrve32.Uart()
emu.map_device(0x1000_0000, uart)              # QEMU virt layout: existing drivers work
emu.map_device(0x0010_0000, mrve32.TestFinisher())

class Sensor:                                  # a new peripheral is a Python class
    size = 4
    def read(self, offset, width): return 42
    def write(self, offset, width, value): pass

emu.map_device(0x4000_0000, Sensor())

emu.load_elf("firmware.elf")                   # your firmware, built with GCC or Clang
res = emu.run(10_000_000)
assert res.reason == "exit" and res.exit_code == 0
assert b"Hello" in uart.output()               # a firmware test, ready for pytest
```

### Why MRVE32

- **Test firmware like ordinary code.** Every run is deterministic: the
  same firmware and inputs give the same instructions, cycles and output,
  so firmware tests can live in pytest or `cargo test` and run in CI.
- **Model hardware in Python.** A peripheral is a class with `read` and
  `write`; it can tick with the clock and raise interrupts. Custom
  instructions (`custom-0`/`custom-1`) are a Python method too: try an
  accelerator instruction from the real firmware before writing RTL.
- **Trustworthy core.** It passes the official
  [RISC-V Architectural Certification Tests](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/tests/arch-test/README.md)
  (ACT4, expected values from the Sail golden model) and all 75 applicable
  [riscv-tests](https://github.com/riscv-software-src/riscv-tests) in CI;
  the decoder is fuzzed, and the disassembler matches GNU objdump.
- **Small and embeddable.** A library, not a framework: no configuration
  language, no dependencies beyond Rust `std`. An interpreter, about
  45 million instructions per second on one laptop core
  (`cargo bench --bench mips`).

How it compares: QEMU is far faster and models many real boards, but new
devices are C code compiled into QEMU. Spike is the reference ISA
simulator, extended in C++. Renode simulates whole multi-node systems with
its own platform description files. MRVE32 aims at the space between:
one microcontroller, scriptable from Python, easy to extend and to embed
in a test suite.

**Not a fit (yet)** if you need Linux or an RTOS that requires
User/Supervisor mode, several harts, floating point (F/D), or a model of a
specific vendor chip.

```bash
pip install mrve32                # Python (wheels for Linux, macOS, Windows)
cargo install --git https://github.com/vladimirmitnovitskiy/MRVE32   # the `mrve32` CLI
mrve32 firmware.elf               # UART -> stdout, exit status from the firmware
```

Documentation: [Python API](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/python.md) ·
[writing devices](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/devices.md) ·
[command-line runner](#command-line-runner) · [Rust usage](#rust-usage) ·
[verification](#verification--quality-assurance)

---

## Key Architecture & Features

- **RV32IMAC + Zicsr + Zifencei**: base integer ISA, multiplication/division, atomics (`lr.w`/`sc.w` and all `amo*.w`), compressed 16-bit instructions (C/Zca), CSR instructions, `fence`/`fence.i`. C can be switched off (`Cpu::set_compressed(false)`, `--isa rv32ima`, `Emulator(compressed=False)`) to model a core without it, with IALIGN = 32 and `misa.C` following it.
- **Machine-mode privileged architecture**: precise synchronous exceptions (illegal instruction, misaligned jump target, instruction/load/store access faults, `ecall`, `ebreak`) delivered through `mtvec` with `mepc`/`mcause`/`mtval`/`mstatus` updated as hardware does; `mret`.
- **Interrupts & timer**: machine external, software and timer interrupts with spec priority (MEI > MSI > MTI), direct and vectored `mtvec`, `wfi` that stalls the hart until an interrupt is pending. A CLINT device (`msip`, `mtime`, `mtimecmp`, SiFive memory map) is included; any device can raise interrupt lines through `BusDevice::irq_lines`.
- **Counters (Zicntr)**: 64-bit `mcycle`/`minstret` with `mcountinhibit`, read-only `cycle`/`instret` shadows; `mhpmcounter*`/`mhpmevent*` are hardwired to zero.
- **Strict CSR file**: accesses to unimplemented CSRs and writes to read-only CSRs raise illegal-instruction exceptions; WARL fields (`mstatus`, `mtvec`, `mepc`, `mie`, `mip`) are masked. Implemented: `mstatus`, `mstatush`, `misa`, `mie`, `mip`, `mtvec`, `mscratch`, `mepc`, `mcause`, `mtval`, `mcountinhibit`, `mcycle[h]`, `minstret[h]`, `cycle[h]`, `instret[h]`, `mvendorid`, `marchid`, `mimpid`, `mhartid`, `mconfigptr`.
- **Tested against riscv-tests**: passes all 75 applicable tests from [`riscv-tests`](https://github.com/riscv-software-src/riscv-tests) (`rv32ui-p-*`, `rv32um-p-*`, `rv32ua-p-*`, `rv32uc-p-rvc` and the M-mode `rv32mi-p-*` tests); every test except `rvc` runs both with and without C. The [RISC-V Architectural Certification Tests](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/tests/arch-test/README.md) (`riscv-arch-test`, ACT4) have a device configuration for MRVE32: the 104 unprivileged tests for RV32IMAC agree with QEMU at every signature point, and CI runs the full Sail-based flow.
- **Bare-Metal & Deterministic**: Machine mode only, no MMU, virtual memory, or OS overhead.
- **Modular Bus Architecture**: Devices, RAM, and memory-mapped peripherals implement the `BusDevice` trait. The bus performs offset translation and address routing.
- **Hardware Coprocessor Slot**: extension point via `trait Coprocessor` (or a Python object, `Emulator.set_coprocessor`) for custom instructions (opcodes `0x0B` and `0x2B`). Register updates are committed atomically and `x0` stays zero.
- **Unidirectional Ownership**: Clean Rust ownership model (`cpu.step(&mut bus)`). The CPU core maintains zero references to the system bus.
- **No dependencies**: the core library depends only on Rust `std`.
- **Standard peripherals**: RAM, CLINT, an NS16550A-compatible UART and a `sifive,test0` test finisher (firmware sets the exit status), in the QEMU `virt` register layout so existing drivers work unchanged.
- **Command-line runner**: `mrve32 firmware.elf` runs an ELF on a QEMU-virt-like memory map, streams the UART to stdout and exits with the firmware's status; `--trace` prints every instruction (GNU objdump-style disassembler), `--stop-on-trap`, `--max-steps`, double-fault detection; `--htif` and `--signature` for compliance suites (HTIF `tohost`, Sail-format signature dump).
- **Pluggable devices**: a device is anything implementing `BusDevice` (Rust), a Python object with `read`/`write` methods, or a native module built separately against the device C ABI ([`include/mrve32_device.h`](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/include/mrve32_device.h), `mrve32::plugin`). Devices get `tick()` time and can raise interrupt lines. See [`docs/devices.md`](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/devices.md).
- **Python package (PyO3)**: `pip install`-able `mrve32` module with an `Emulator` class (ELF loading, symbols, breakpoints, run control, instruction tracing, memory/register/CSR access, `map_device`), `Uart`, `TestFinisher` and `disassemble`. Typed (`py.typed` stubs).

### Device modules

| Module | Description |
|---|---|
| [`mrve32-snn`](https://github.com/vladimirmitnovitskiy/MRVE32/tree/main/modules/snn) | SNN layer accelerator model (draft v0): LIF/IF neurons, `i16` potentials, `i8` weights, a bit-exact golden model behind a swappable `SnnBackend`, an analytical cycle model, a completion interrupt, and a NumPy reference. [Spec](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/modules/snn/docs/snn-accelerator-v0.md). |

Modules live in `modules/` for now but depend on the core only through its public API and could be moved to their own repositories unchanged.

### Not implemented (yet)

The `time`/`timeh` CSRs (read `mtime` from the CLINT instead), an interrupt controller (PLIC; the UART drives MEIP directly), User/Supervisor modes, PMP, debug triggers, semihosting, the F/D extensions, and Zcb/Zcmp. Atomics assume a single hart: `aq`/`rl` have no effect and a trap clears the LR reservation.
Build firmware with `-march=rv32imac_zicsr_zifencei -mabi=ilp32` (or any subset, e.g. `rv32imc_zicsr`). Misaligned loads and stores are performed in hardware rather than trapped.

---

## Command-line runner

```bash
cargo install --path .           # installs the `mrve32` binary
mrve32 firmware.elf              # UART -> stdout, exit status from the finisher
mrve32 --trace --max-steps 20 firmware.elf
mrve32 --htif --signature out.sig test.elf  # Sail/Spike-style tests: HTIF tohost, signature dump
mrve32 --help
```

Default memory map (QEMU `virt`-compatible, every address configurable):

| Device | Base | Notes |
|---|---|---|
| RAM | `0x8000_0000` | `--ram-size`, default 128 MiB |
| UART (NS16550A) | `0x1000_0000` | output to stdout; `--input FILE\|-` feeds its receiver |
| Test finisher (`sifive,test0`) | `0x0010_0000` | write `0x5555` to pass, `(code << 16) \| 0x3333` to fail |
| CLINT | `0x0200_0000` | `mtime`, `mtimecmp`, `msip` |

Exit status is the firmware's (0–255), or 123 (idle in `wfi` forever),
124 (`--max-steps`), 125 (usage/load error), 126 (trap with
`--stop-on-trap`, or a double fault such as running without `mtvec`).
[`tests/firmware`](https://github.com/vladimirmitnovitskiy/MRVE32/tree/main/tests/firmware) is a minimal freestanding C example
(startup, linker script, UART, finisher) built by
`scripts/build-test-firmware.sh`.

---

## Architecture Overview

```text
        +------------------------------------+
        |        CPU Core (RV32IMAC)         |
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
state in `BusDevice::tick`; see [`docs/devices.md`](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/devices.md).

---
## Python Testbenches (PyO3)

Install from PyPI (prebuilt wheels for Linux, macOS and Windows, Python >= 3.10):

```bash
pip install mrve32            # the SNN device module: pip install mrve32-snn
```

From a checkout, `pip install -e ".[test]"` builds the extension with
[maturin](https://www.maturin.rs/) (needs a Rust toolchain).

The full reference with more examples is [docs/python.md](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/python.md).
A short tour:

Run firmware and inspect the machine:

```python
import mrve32

# RAM at 0x8000_0000 and a CLINT at 0x0200_0000 by default
# (clint_base=None omits it); add a UART and a test finisher as in QEMU virt.
emu = mrve32.Emulator(ram_size=1 << 20)
uart = mrve32.Uart()
emu.map_device(0x1000_0000, uart)
emu.map_device(0x0010_0000, mrve32.TestFinisher())

entry = emu.load_elf("firmware.elf")      # path or bytes; sets pc to the entry
res = emu.run(1_000_000, stop_on_trap=True)
print(res.reason, res.steps)              # "exit" | "max_steps" | "breakpoint" | "trap" | "idle"
print(res.exit_code, uart.output())       # finisher status, bytes sent on the UART
if res.trap:
    print(hex(res.trap.cause), res.trap.description)

print(emu.regs, hex(emu.pc), emu.cycles, emu.instret)
emu.add_breakpoint(emu.symbol("main"))
emu.set_tracer(lambda pc, word, trap: print(hex(pc), mrve32.disassemble(word, pc) if word else trap))
data = emu.read_bytes(emu.symbol("buffer"), 64)  # raises mrve32.BusError if unmapped
```

Attach devices: native modules or plain Python objects ([`docs/devices.md`](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/devices.md)):

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

Prototype custom instructions (custom-0 `0x0B` / custom-1 `0x2B`) with a Python
coprocessor; register writes are committed only if `execute` succeeds:

```python
class Mac:
    def execute(self, call):              # call: mrve32.CoprocessorCall
        if call.funct3 != 0:
            return False                  # illegal instruction trap
        acc = call.read_reg(call.rd) + call.read_reg(call.rs1) * call.read_reg(call.rs2)
        call.write_reg(call.rd, acc & 0xFFFF_FFFF)
        # call.read_u32(addr) / call.write_u32(addr, v): bus access
        # (an uncaught BusError becomes a load/store access fault);
        # call.next_pc = target: branch

emu.set_coprocessor(Mac())                # None detaches it
```

For example, with the SNN module (`pip install mrve32-snn`):

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

RISC-V Architectural Certification Tests (needs a RISC-V GCC; `--qemu` also
runs every self-checking ELF on QEMU; the Sail-based flow is described in
[`tests/arch-test/README.md`](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/tests/arch-test/README.md)):

```bash
scripts/arch-test.sh local --qemu
```

Robustness: `tests/decoder_fuzz.rs` runs every 16-bit encoding, every
opcode/funct3/funct7 combination and a million random words through the
decoders and the disassembler, and random programs through the CPU.
Coverage-guided targets for `cargo-fuzz` are in [`fuzz/`](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/fuzz/README.md)
(CI runs them weekly). The disassembler was cross-checked against GNU objdump
2.46: the only differences are encodings the spec reserves on RV32
(`slli`/`srli`/`srai` with shamt[5] = 1, `c.addi16sp` with immediate 0),
which objdump decodes and MRVE32 rejects, and the names of C HINT encodings.

Interpreter throughput (MIPS) for a few instruction mixes:

```bash
cargo bench --bench mips
```

Run static analysis and linter checks:

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The minimum supported Rust version (MSRV) is **1.88** (checked in CI).
Changes are listed in [`CHANGELOG.md`](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/CHANGELOG.md).
---
## License

Licensed under Apache License, Version 2.0.
