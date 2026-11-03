# Python API

The `mrve32` package runs RISC-V firmware from Python. The emulator core
(CPU, bus, devices, ELF loader) is Rust; Python drives it and can add
devices and custom instructions. Typical uses are testbenches for firmware,
behavioural models of peripherals, and prototyping hardware accelerators
before writing RTL.

```bash
pip install mrve32              # the emulator
pip install mrve32-snn          # optional: the SNN accelerator device
```

Wheels exist for Linux (x86_64, aarch64), macOS (x86_64, arm64) and
Windows (x64), CPython 3.10 and newer. Elsewhere `pip` builds from source
and needs a Rust toolchain. The package ships type stubs (`py.typed`), so
editors and mypy see every signature below.

Contents:

- [Quick start](#quick-start)
- [Concepts](#concepts): memory map, steps and cycles, traps, why a run stops
- [`Emulator`](#emulator): construction, execution, loading, memory, CPU state
- [`RunResult` and `Trap`](#runresult-and-trap)
- [Built-in devices](#built-in-devices): `Ram`, `Clint`, `Uart`, `TestFinisher`
- [Devices in Python](#devices-in-python)
- [Custom instructions (coprocessors)](#custom-instructions-coprocessors)
- [Debugging](#debugging): breakpoints, tracing, disassembly
- [Errors](#errors)
- [`mrve32-snn`](#mrve32-snn)

## Quick start

A complete program, without a cross-compiler: four instructions that tell
the test finisher "pass".

```python
import mrve32

emu = mrve32.Emulator()                        # 64 KiB RAM at 0x8000_0000, CLINT
emu.map_device(0x0010_0000, mrve32.TestFinisher())

emu.load_program(0x8000_0000, [
    0x001002B7,  # lui  t0, 0x100        t0 = 0x0010_0000 (the finisher)
    0x00005337,  # lui  t1, 0x5
    0x55530313,  # addi t1, t1, 0x555    t1 = 0x5555 ("pass")
    0x0062A023,  # sw   t1, 0(t0)
])

res = emu.run(1000)
print(res.reason, res.exit_code, res.steps)    # exit 0 4
print(hex(emu.read_reg(6)))                    # 0x5555 (t1 = x6)
```

With real firmware, build an ELF (for example with
`-march=rv32imac_zicsr -mabi=ilp32 -nostdlib`, linked at `0x8000_0000`)
and load it instead:

```python
emu = mrve32.Emulator(ram_size=1 << 20)
uart = mrve32.Uart()
emu.map_device(0x1000_0000, uart)              # same map as QEMU virt
emu.map_device(0x0010_0000, mrve32.TestFinisher())

emu.load_elf("firmware.elf")                   # sets pc to the entry point
res = emu.run(10_000_000)
print(res.reason, res.exit_code)
print(uart.output().decode())                  # what the firmware printed
```

## Concepts

### Memory map

An `Emulator` starts with RAM and a CLINT; every other device is added with
[`map_device`](#devices). The defaults follow QEMU's `virt` machine, so
firmware written for it runs unchanged:

| Device | Default base | Size | How to get it |
|---|---|---|---|
| RAM | `0x8000_0000` | 64 KiB | `Emulator(ram_base=..., ram_size=...)` |
| CLINT (timer, software interrupt) | `0x0200_0000` | 64 KiB | `Emulator(clint_base=...)`, `None` to omit |
| UART (NS16550A) | `0x1000_0000` | 256 B | `map_device(0x1000_0000, Uart())` |
| Test finisher | `0x0010_0000` | 4 KiB | `map_device(0x0010_0000, TestFinisher())` |

Device windows must not overlap. An access to an address no device covers
is an access fault for the CPU and a `BusError` for Python.

### Steps, cycles and instructions

One **step** is one clock cycle. Every instruction takes one cycle, so a
step normally executes one instruction. While the hart sleeps in `wfi`, a
step executes nothing but still advances time: devices `tick`, `mcycle`
counts, `minstret` does not.

- `emu.cycles` is `mcycle`, `emu.instret` is `minstret` (both 64-bit).
- `RunResult.steps` is the number of cycles a `run()` call executed.
- The CLINT's `mtime` advances once per cycle (or every `divider` cycles,
  see [`Clint`](#clint)).

### Traps

The emulated hart is an RV32IMAC machine-mode core. Exceptions and
interrupts behave as on hardware: `mepc`, `mcause`, `mtval` and `mstatus`
are updated and `pc` jumps to `mtvec`. `step()` and `run()` report the trap
**after** it was taken, so `emu.pc` already points at the handler.

`mtvec` is 0 after reset, and nothing is mapped at address 0. Firmware
that can trap must set `mtvec` first, or the first trap jumps to 0 and
faults again (an instruction access fault, `mcause` 1) forever. In a quick
experiment without a handler, use `run(..., stop_on_trap=True)` to stop at
the first trap instead.

Common `mcause` values:

| `mcause` | Meaning | `mtval` |
|---|---|---|
| 0 | Instruction address misaligned | target address |
| 1 | Instruction access fault | address |
| 2 | Illegal instruction | the instruction |
| 3 | Breakpoint (`ebreak`) | pc |
| 4 / 6 | Load / store address misaligned (atomics only) | address |
| 5 / 7 | Load / store access fault | address |
| 11 | `ecall` from M-mode | 0 |
| `0x8000_0003` | Machine software interrupt (CLINT `msip`) | 0 |
| `0x8000_0007` | Machine timer interrupt (CLINT `mtimecmp`) | 0 |
| `0x8000_000B` | Machine external interrupt (devices) | 0 |

Misaligned ordinary loads and stores to RAM do not trap. They are performed
in hardware.

### Why a run stops

`run(max_steps)` returns a [`RunResult`](#runresult) whose `reason` is one
of:

| `reason` | What happened |
|---|---|
| `"exit"` | A device asked to stop, e.g. the firmware wrote to the `TestFinisher`. `exit_code` holds the status. |
| `"max_steps"` | `max_steps` cycles ran. Call `run()` again to continue. |
| `"breakpoint"` | `pc` reached a [breakpoint](#breakpoints). The instruction there has not executed yet. |
| `"trap"` | A trap was taken and `stop_on_trap=True`. `trap` holds it. |
| `"idle"` | The hart is in `wfi` and no enabled interrupt can ever wake it. |

## `Emulator`

```python
Emulator(
    ram_base: int = 0x8000_0000,
    ram_size: int = 64 * 1024,
    *,
    clint_base: int | None = 0x0200_0000,
    compressed: bool = True,
)
```

A machine: one RV32IMAC hart, a bus with RAM and (unless `clint_base` is
`None`) a CLINT. `pc` starts at `ram_base`. With `compressed=False` the core
is RV32IMA: the C extension is off (`misa.C` = 0), and a jump to an address
that is only 2-byte aligned raises an instruction-address-misaligned trap.

An `Emulator` belongs to the thread that created it.

| Attribute | |
|---|---|
| `ram_base: int` | read-only |
| `clint_base: int \| None` | read-only |
| `compressed: bool` | read/write; the same switch as the constructor argument |
| `devices: list[tuple[int, int, str]]` | `(base, size, type name)` of every mapped device, by address |

`repr(emu)` shows `pc` and the devices:
`Emulator(pc=0x80000000, devices=[Clint@0x02000000, Ram@0x80000000])`.

### Execution

**`step() -> Trap | None`**
Executes one cycle. Returns the trap taken in this cycle, if any.

**`run(max_steps: int, *, stop_on_trap: bool = False) -> RunResult`**
Runs up to `max_steps` cycles. Stops earlier on a device exit request, a
breakpoint, a trap (with `stop_on_trap`) or when the hart is idle forever;
see [why a run stops](#why-a-run-stops). Without `stop_on_trap`, traps are
taken and execution continues in the handler, as on hardware.

**`tick(cycles: int) -> None`**
Advances the devices by `cycles` without executing instructions. Use it to
drive a peripheral directly from Python, e.g. to let an accelerator finish
a job started by a host write.

### Loading

**`load_elf(source: str | os.PathLike | bytes | bytearray) -> int`**
Loads a 32-bit RISC-V ELF from a path or from bytes: copies its loadable
segments onto the bus, sets `pc` to the entry point and returns it. Raises
`ElfError` if the file is not a valid RV32 ELF or a segment does not fit in
mapped memory.

**`symbol(name: str) -> int | None`**
The address of a symbol of the last loaded ELF.

**`symbols: dict[str, int]`**
All symbols of the last loaded ELF.

**`load_program(addr: int, program: list[int]) -> None`**
Writes 32-bit words starting at `addr`. Handy for small hand-assembled
programs; `pc` is not changed.

### Memory

These go through the bus, exactly like accesses from the CPU, so they
reach devices too (reading a UART register from Python has the same side
effects as reading it from firmware). Values are little-endian. Failures
raise `BusError`.

| Method | |
|---|---|
| `read_u8(addr)`, `read_u16(addr)`, `read_u32(addr) -> int` | |
| `write_u8(addr, val)`, `write_u16(addr, val)`, `write_u32(addr, val)` | `val` must fit the width |
| `read_bytes(addr, size) -> bytes` | byte by byte |
| `write_bytes(addr, data: bytes \| bytearray)` | byte by byte |

To exchange arrays with firmware, use NumPy:
`emu.write_bytes(addr, arr.astype("<i2").tobytes())` and
`np.frombuffer(emu.read_bytes(addr, 2 * n), dtype="<i2")`.

### CPU state

| Member | |
|---|---|
| `read_reg(reg) -> int`, `write_reg(reg, val)` | `x0`..`x31` by number; writes to `x0` are ignored; `IndexError` outside 0..31 |
| `regs: list[int]` | a snapshot of all 32 registers |
| `pc: int` | read/write |
| `read_csr(csr) -> int`, `write_csr(csr, val)` | by CSR number; `ValueError` for an unimplemented or read-only CSR |
| `cycles: int` | `mcycle` |
| `instret: int` | `minstret` |
| `is_waiting: bool` | the hart is sleeping in `wfi` |

Register numbers follow the ABI: `ra`=1, `sp`=2, `t0`..`t2`=5..7,
`s0`/`fp`=8, `s1`=9, `a0`..`a7`=10..17, `s2`..`s11`=18..27,
`t3`..`t6`=28..31.

Useful CSR numbers: `mstatus` 0x300, `misa` 0x301, `mie` 0x304, `mtvec`
0x305, `mscratch` 0x340, `mepc` 0x341, `mcause` 0x342, `mtval` 0x343,
`mip` 0x344, `mcycle` 0xB00, `minstret` 0xB02, `mhartid` 0xF14.

All integers are unsigned 32-bit (64-bit for the counters). Passing a
negative number or one that does not fit raises `OverflowError`; write
`-1 & 0xFFFF_FFFF` for all ones, and convert a register to signed with
`v - (1 << 32) if v >> 31 else v`.

## `RunResult` and `Trap`

### `RunResult`

| Attribute | |
|---|---|
| `reason: str` | `"exit"`, `"max_steps"`, `"breakpoint"`, `"trap"` or `"idle"` |
| `steps: int` | cycles executed by this `run()` call |
| `exit_code: int \| None` | the device's exit status when `reason == "exit"` |
| `trap: Trap \| None` | the trap when `reason == "trap"` |

### `Trap`

A trap that has already been taken (the CSRs below hold the same values).

| Attribute | |
|---|---|
| `cause: int` | `mcause`; bit 31 set for interrupts |
| `is_interrupt: bool` | |
| `epc: int` | `mepc`: the faulting instruction, or where an interrupt struck |
| `tval: int` | `mtval`: address or instruction, see [Traps](#traps) |
| `description: str` | e.g. `"illegal instruction: 0x00000000"` |

## Built-in devices

Native devices are written in Rust and run at full speed. Create one, then
pass it to `map_device`; its window size is fixed. A device object can be
mapped only once (`RuntimeError` otherwise).

### `Ram`

`Ram(size: int)`: zero-initialised memory of `size` bytes. The emulator
already has one; map more for separate regions (e.g. a "flash" at another
address).

### `Clint`

`Clint(divider: int = 1)`: the SiFive core-local interruptor: `msip` at
offset 0, `mtimecmp` at 0x4000, `mtime` at 0xBFF8. `mtime` advances by one
every `divider` cycles. It raises the machine timer interrupt while
`mtime >= mtimecmp` and the software interrupt while `msip` is 1. The
`Emulator` maps one unless `clint_base=None`, so you only need this class to
place it yourself.

### `Uart`

`Uart(*, reg_shift: int = 0, echo: bool = False)`: an NS16550A subset as in
QEMU `virt`: transmit, receive, line status, interrupt enable (the receive
and transmit interrupts drive the machine external interrupt). Registers
are `1 << reg_shift` bytes apart. The object keeps working after it is
mapped, so the testbench can talk to the firmware:

| Member | |
|---|---|
| `output() -> bytes` | everything transmitted so far |
| `take_output() -> bytes` | transmitted since the last call, then clears |
| `write_input(data: bytes)` | queue bytes for the firmware to receive |
| `pending_input: int` | queued bytes not read yet |

With `echo=True`, transmitted text is also written to `sys.stdout` line by
line.

```python
uart = mrve32.Uart()
emu.map_device(0x1000_0000, uart)
uart.write_input(b"help\n")
emu.run(1_000_000)
assert b"usage" in uart.take_output()
```

### `TestFinisher`

`TestFinisher()`: the `sifive,test0` device. The firmware writes `0x5555`
for "pass" (exit code 0) or `(code << 16) | 0x3333` for "fail" with `code`;
`run()` then returns `reason == "exit"` with that `exit_code`.

### Module constants

- `mrve32.__version__`: the package version.
- `mrve32.DEVICE_ABI_VERSION`: version of the C ABI for native device
  modules (`include/mrve32_device.h`).

## Devices in Python

Any object with `read` and `write` methods can be mapped. This is the
fastest way to model a peripheral; for speed, or to ship a device as its
own package, write a native module instead (see
[docs/devices.md](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/devices.md)).

```python
class Device(Protocol):            # mrve32.Device
    def read(self, offset: int, width: int) -> int: ...
    def write(self, offset: int, width: int, value: int) -> None: ...
    # optional:
    # def tick(self, cycles: int) -> None: ...
    # def irq_lines(self) -> int: ...
    # def exit_request(self) -> int | None: ...
```

| Method | Called | |
|---|---|---|
| `read(offset, width)` | on a load | `offset` is relative to the window base; `width` is 1, 2 or 4; return a value that fits in `width` bytes |
| `write(offset, width, value)` | on a store | |
| `tick(cycles)` | after every step, and by `Emulator.tick` | |
| `irq_lines()` | before every step | a mask in `mip` layout: `1 << 11` external, `1 << 7` timer, `1 << 3` software; lines of all devices are OR-ed |
| `exit_request()` | after every step | return an exit code to stop `run()` with `reason == "exit"`, otherwise `None` |

The window size is `map_device(base, dev, size=...)` or, if not given, the
object's `size` attribute.

- Raise `mrve32.BusError` from `read`/`write` for an access fault the
  firmware should see (e.g. an unmapped register).
- Any other exception is treated as a bug in the model: `step()`/`run()`
  stops and re-raises it.
- `tick`, `irq_lines` and `exit_request` cross from Rust into Python on
  every step. Define them only if the device needs them.

An example: a countdown timer that raises the external interrupt.

```python
import mrve32

class Countdown:
    size = 4

    def __init__(self):
        self.left = 0
        self.armed = False

    def read(self, offset, width):
        if offset != 0 or width != 4:
            raise mrve32.BusError("Countdown: only a 32-bit register at 0")
        return self.left

    def write(self, offset, width, value):
        if offset != 0 or width != 4:
            raise mrve32.BusError("Countdown: only a 32-bit register at 0")
        self.left, self.armed = value, value != 0

    def tick(self, cycles):
        self.left = max(0, self.left - cycles)

    def irq_lines(self):
        return 1 << 11 if self.armed and self.left == 0 else 0  # MEIP

emu = mrve32.Emulator()
timer = Countdown()
emu.map_device(0x4000_0000, timer)
emu.map_device(0x0010_0000, mrve32.TestFinisher())

emu.load_program(0x8000_0000, [
    0x10500073,  # wfi              sleep until the interrupt
    0xFFDFF06F,  # j    .-4
])
emu.load_program(0x8000_0040, [  # the interrupt handler
    0x400002B7,  # lui  t0, 0x40000
    0x0002A023,  # sw   zero, 0(t0)  disarm the timer
    0x001002B7,  # lui  t0, 0x100
    0x00005337,  # lui  t1, 0x5
    0x55530313,  # addi t1, t1, 0x555
    0x0062A023,  # sw   t1, 0(t0)    finisher: pass
])
emu.write_csr(0x305, 0x8000_0040)   # mtvec = handler
emu.write_csr(0x304, 1 << 11)       # mie.MEIE
emu.write_csr(0x300, 1 << 3)        # mstatus.MIE
emu.write_u32(0x4000_0000, 100)     # interrupt in 100 cycles

res = emu.run(10_000)
print(res.reason, emu.read_csr(0x342) == 0x8000_000B)   # exit True
```

## Custom instructions (coprocessors)

The `custom-0` (opcode `0x0B`) and `custom-1` (`0x2B`) instruction spaces
are reserved for vendor extensions. Attach a coprocessor to give them a
meaning, e.g. to try out an accelerator instruction before building it:

**`Emulator.set_coprocessor(coprocessor | None)`**: `coprocessor` is any
object with `execute(call: CoprocessorCall) -> bool | None` (the
`mrve32.Coprocessor` protocol). `None` detaches it. Without a coprocessor
these opcodes are illegal instructions.

`execute` is called for each custom instruction. Return `None` or `True`
when it executed, `False` to raise an illegal-instruction trap (an encoding
you do not implement).

`CoprocessorCall` describes the instruction and gives access to the hart:

| Member | |
|---|---|
| `opcode`, `word`, `pc` | `0x0B`/`0x2B`, the 32-bit instruction, its address |
| `rd`, `rs1`, `rs2`, `funct3`, `funct7` | R-type fields, decoded for you (use `word` for other formats) |
| `read_reg(reg)`, `write_reg(reg, value)` | registers; writes to `x0` are ignored |
| `next_pc` | read/write; `pc + 4` by default, set it to branch |
| `read_u8/16/32(addr)`, `write_u8/16/32(addr, value)` | bus access, as from the CPU |

Register writes and `next_pc` take effect only if `execute` succeeds, so a
trapping instruction leaves no partial result. Memory writes are not undone.
If a bus access raises `BusError` and `execute` does not catch it, the
instruction raises a load/store access fault instead. Any other exception
stops `run()` and is re-raised. `call` is valid only during `execute`.

A multiply-accumulate instruction, `mac rd, rs1, rs2` (`rd += rs1 * rs2`),
in custom-0 with `funct3 = 0`:

```python
class Mac:
    def execute(self, call):
        if call.funct3 != 0:
            return False                                  # illegal instruction
        acc = call.read_reg(call.rd) + call.read_reg(call.rs1) * call.read_reg(call.rs2)
        call.write_reg(call.rd, acc & 0xFFFF_FFFF)

emu = mrve32.Emulator()
emu.set_coprocessor(Mac())
emu.write_reg(10, 5)                       # a0
emu.write_reg(11, 6)                       # a1
emu.write_reg(12, 7)                       # a2
emu.load_program(0x8000_0000, [0x00C5850B])  # mac a0, a1, a2 (.insn r 0x0B, 0, 0, a0, a1, a2)
emu.step()
print(emu.read_reg(10))                    # 47
```

In C firmware, emit it with the assembler's `.insn` directive:
`asm volatile(".insn r 0x0B, 0, 0, %0, %1, %2" : "+r"(acc) : "r"(a), "r"(b));`

## Debugging

### Breakpoints

| Method | |
|---|---|
| `add_breakpoint(pc)` | `run()` stops with `reason == "breakpoint"` before executing the instruction at `pc` |
| `remove_breakpoint(pc) -> bool` | `False` if there was none |
| `clear_breakpoints()` | |
| `breakpoints: list[int]` | |

`run()` checks for breakpoints after each step, so calling it again from a
breakpoint executes that instruction and continues. For the same reason a
breakpoint at the `pc` where `run()` starts is not hit on that call. A
breakpoint does not stop `step()`.

```python
emu.load_elf("firmware.elf")
emu.add_breakpoint(emu.symbol("main"))
res = emu.run(1_000_000)
assert res.reason == "breakpoint" and emu.pc == emu.symbol("main")
```

### Tracing

**`set_tracer(tracer | None)`**: calls `tracer(pc, word, trap)` after every
step: `pc` of the instruction, `word` the instruction (`None` if the fetch
faulted or the hart slept), `trap` the trap taken or `None`. An exception in
the tracer stops `run()` and is re-raised. Tracing calls into Python on
every step, so it slows the run down a lot.

```python
def trace(pc, word, trap):
    if trap:
        print(f"{pc:08x}  trap: {trap.description}")
    elif word is not None:
        print(f"{pc:08x}  {mrve32.disassemble(word, pc)}")

emu.set_tracer(trace)
```

### Disassembly

**`mrve32.disassemble(word: int, pc: int | None = None) -> str`**: one
instruction in GNU `objdump` style, with its pseudo-instructions and CSR
names. A word whose low two bits are not `0b11` is a 16-bit compressed
instruction. With `pc`, branch and jump targets are absolute.

```python
mrve32.disassemble(0x00A58533)              # 'add a0, a1, a0'
mrve32.disassemble(0x4515)                  # 'li a0, 5'
mrve32.disassemble(0x0000006F, 0x8000_0000) # 'j 0x80000000'
```

## Errors

| Exception | Raised by |
|---|---|
| `mrve32.BusError` | a memory access from Python to an unmapped address, or one a device rejects (wrong width, busy) |
| `mrve32.ElfError` | `load_elf` with an invalid file or segments outside mapped memory |
| `ValueError` | overlapping device windows; an unimplemented or read-only CSR; invalid constructor arguments |
| `TypeError` | `map_device` with an object that is not a device |
| `IndexError` | a register number outside 0..31 |
| `OverflowError` | an integer that does not fit (negative, or too wide for the access) |
| `RuntimeError` | mapping a native device twice; using a `CoprocessorCall` after `execute` returned |

Exceptions raised by your own devices, coprocessors or tracer come out of
the `step()`/`run()`/memory call that triggered them, unchanged.

## `mrve32-snn`

A behavioural model of a memory-mapped accelerator for one layer of a
spiking neural network, plus a NumPy reference model and a host-side
driver. The register-level specification is
[snn-accelerator-v0.md](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/modules/snn/docs/snn-accelerator-v0.md).

```python
import numpy as np
import mrve32
from mrve32_snn import SnnAccelerator, SnnDriver, reference_step

emu = mrve32.Emulator()
emu.map_device(0x1000_0000, SnnAccelerator())
dev = SnnDriver(emu, 0x1000_0000)

rng = np.random.default_rng(0)
weights = rng.integers(-128, 128, size=(64, 32), dtype=np.int8)   # [n_in, n_out]
dev.load_layer(weights, v_th=100, leak_shift=4)

vmem = np.zeros(32, np.int16)
for t in range(10):
    spikes = rng.random(64) < 0.2
    out = dev.step(spikes)                    # write inputs, START, wait, read outputs
    expected, vmem = reference_step(spikes, weights, vmem, v_th=100, leak_shift=4)
    assert (out == expected).all() and (dev.read_vmem() == vmem).all()
print(dev.cycles, "cycles for the last step")
```

`SnnDriver` drives the device from Python through the emulated bus, with
no firmware: useful to test models and data. Firmware programs the same
registers itself; the driver's code is a readable reference for the
programming sequence.

### `SnnAccelerator`

```python
SnnAccelerator(*, max_inputs=1024, max_outputs=512, weight_bytes=131072,
               t_setup=8, t_neuron=1, t_syn=1, lanes=8)
```

The device; map it with `Emulator.map_device` (a 1 MiB window). The
arguments set its capacity and its timing model (how many cycles a step
takes, spec §6). It raises the machine external interrupt when a step
finishes, if enabled.

### `SnnDriver(emu, base)`

Raises `ValueError` if there is no accelerator at `base`. Attributes:
`max_inputs`, `max_outputs`, `weight_bytes` (read from the device), and
`n_in`, `n_out` (the configured layer).

| Method | |
|---|---|
| `load_layer(weights, *, v_th, leak_shift=0, bias=None)` | write `int8[n_in, n_out]` weights (and optional `int16[n_out]` bias) and configure the layer from their shape |
| `configure(n_in, n_out, *, v_th, leak_shift=0, bias=False)` | set the layer size and neuron parameters only |
| `write_bias(bias)`, `write_vmem(vmem)`, `read_vmem(n=None)` | bias and membrane potentials, `int16` |
| `clear_vmem()` | reset all potentials to 0 |
| `write_input_spikes(spikes)`, `read_output_spikes(n=None)` | boolean vectors |
| `start(*, clear_vmem=False)` | start one timestep |
| `wait(*, max_cycles=10_000_000, chunk=64)` | `Emulator.tick` until done; raises `SnnError` on a configuration error, `TimeoutError` if it never finishes |
| `step(spikes_in) -> bool[n_out]` | write inputs, `start`, `wait`, acknowledge, return output spikes |
| `enable_irq(enabled=True)`, `ack_irq()` | the completion interrupt |
| `status`, `busy`, `cycles`, `in_spikes`, `out_spikes` | status registers; `cycles` is the duration of the last step |

### Functions

- `reference_step(spikes_in, weights, vmem, *, v_th, leak_shift=0, bias=None)
  -> (spikes_out, vmem_out)`: one timestep in NumPy, bit-exact with the
  device (spec §5). Use it as the golden model in tests.
- `pack_spikes(spikes) -> uint32[]` and `unpack_spikes(words, n) -> bool[n]`:
  convert between boolean vectors and the device's packed format (neuron
  `k` is bit `k % 32` of word `k // 32`).

`SnnError(code)` is raised for a configuration error; `code` is one of
`ERR_INVALID_LAYER_SIZE`, `ERR_WEIGHTS_DO_NOT_FIT`, `ERR_INVALID_THRESHOLD`.
The module also exports the register offsets and bit masks of the
specification (`REG_CTRL`, `REG_STATUS`, `WEIGHTS_BASE`, `CTRL_START`,
`STATUS_DONE`, ...) for firmware-style access with `emu.read_u32` and
`emu.write_u32`.
