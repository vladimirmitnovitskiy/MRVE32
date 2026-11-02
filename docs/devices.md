# Writing device modules

The MRVE32 core is a CPU, a bus, RAM, a CLINT and an ELF loader. Every other
peripheral is a **device module**: a memory-mapped device the emulator
knows nothing about in advance. The SNN accelerator in
[`modules/snn`](../modules/snn) is such a module and uses only the public
interfaces described here, so it doubles as a full worked example.

A device is a window of `size` bytes on the bus. It gets:

| call | when |
|---|---|
| `read(offset, width)` / `write(offset, width, value)` | a load/store from the CPU or a host access; `offset` is relative to the window start, `width` is 1, 2 or 4, values are little-endian |
| `tick(cycles)` | after every CPU step (`cycles` = 1), or from `Emulator.tick(n)` |
| `exit_request()` | after every step; return an exit code to stop the run (`StopReason::Exit`, `reason == "exit"`), as the test finisher does |
| `irq_lines()` | before every CPU step; return a mask in `mip` layout: `1 << 11` external (MEIP), `1 << 7` timer (MTIP), `1 << 3` software (MSIP). Lines of all devices are OR-ed |

A failed access becomes a precise load/store access fault in the CPU
(`mcause` 5/7, `mtval` = address) and a `mrve32.BusError` for host accesses.

There are three ways to write one. Pick by where the device code lives.

## 1. Rust, in the same program

Implement `mrve32::BusDevice` and map it. Use this from Rust testbenches,
or when you embed the emulator in a Rust application.

```rust
use mrve32::{Bus, BusDevice, BusError, Reg};
use mrve32::cpu::csr::MIP_MEIP;

/// A countdown timer: write N to offset 0, interrupt (MEIP) after N cycles;
/// writing 0 disarms it.
#[derive(Default)]
struct Countdown {
    left: u32,
    armed: bool,
}

impl BusDevice for Countdown {
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        match addr {
            0 => Ok(self.left),
            _ => Err(BusError::AddressNotFound(addr)),
        }
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        match addr {
            0 => {
                self.left = val;
                self.armed = val != 0;
                Ok(())
            }
            _ => Err(BusError::AddressNotFound(addr)),
        }
    }
    // Only 32-bit accesses are supported.
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> { Err(BusError::InvalidAccess(addr)) }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> { Err(BusError::InvalidAccess(addr)) }
    fn write_u8(&mut self, addr: Reg, _: u8) -> Result<(), BusError> { Err(BusError::InvalidAccess(addr)) }
    fn write_u16(&mut self, addr: Reg, _: u16) -> Result<(), BusError> { Err(BusError::InvalidAccess(addr)) }

    fn tick(&mut self, cycles: u64) {
        self.left = self.left.saturating_sub(cycles as u32);
    }
    fn irq_lines(&self) -> Reg {
        if self.armed && self.left == 0 { MIP_MEIP } else { 0 }
    }
}

let mut bus = Bus::new();
bus.map_device(0x4000_0000, 4, Box::new(Countdown::default())).unwrap();
bus.write_u32(0x4000_0000, 3).unwrap();
bus.tick(3);
assert_eq!(bus.irq_lines(), MIP_MEIP);
```

## 2. Python object

Any object with `read` and `write` methods (the `mrve32.Device` protocol);
`tick`, `irq_lines` and `exit_request` are optional. Fastest way to prototype a peripheral or
a behavioural model in a testbench.

```python
import mrve32

class Uart:
    size = 8                        # or pass size= to map_device

    def __init__(self):
        self.out = bytearray()

    def read(self, offset, width):
        if offset == 4:
            return 1                # status: TX ready
        raise mrve32.BusError("no such register")  # -> access fault

    def write(self, offset, width, value):
        if offset == 0:
            self.out.append(value & 0xFF)

emu = mrve32.Emulator()
uart = Uart()
emu.map_device(0x1000_0000, uart)
```

- Raise `mrve32.BusError` for an access fault the firmware should see.
- Any other exception is a bug in the model: it stops `run()`/`step()` and
  is re-raised from that call.
- `tick`, `irq_lines` and `exit_request` are called on every CPU step, so
  define them only if the device needs them: each call crosses into Python.

## 3. Native module (separate package)

For speed, or to ship a device as its own package (as `mrve32-snn` does).
The emulator and the module are different compiled artifacts, so they meet
at a small, versioned **C ABI** rather than at Rust types:

- [`include/mrve32_device.h`](../include/mrve32_device.h): the
  `mrve32_device` table (`abi_version`, `size`, `ctx`, `read`, `write`,
  `tick`, `irq_lines`, `destroy`, `exit_request`) and status codes.
- `mrve32::plugin` (Rust): the same table as `RawDevice`, an exporter for
  any `BusDevice` (`RawDevice::new`) and the importer (`ForeignDevice`).

In Python, the device object's `__mrve32_device__()` method returns a
`PyCapsule` named `"mrve32.device"` that holds a `mrve32_device *`.
`Emulator.map_device` takes the device out of it (sets `ctx` to NULL); from
then on the emulator owns it and calls `destroy` when it is dropped.

### In Rust (recommended)

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
mrve32 = { version = "0.1", features = ["python-device"] }
pyo3 = { version = "0.30", features = ["extension-module", "abi3-py310"] }
```

```rust,ignore
use mrve32::RawDevice;
use mrve32::plugin::python::into_capsule;
use pyo3::prelude::*;

#[pyclass(unsendable)]
struct Countdown { device: Option<my_device::Countdown> }

#[pymethods]
impl Countdown {
    #[new]
    fn new() -> Self { Self { device: Some(Default::default()) } }

    fn __mrve32_device__<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.take()
            .ok_or_else(|| pyo3::exceptions::PyRuntimeError::new_err("already attached"))?;
        into_capsule(py, RawDevice::new(device, 4))
    }
}
```

Build it with maturin; users then write
`emu.map_device(0x4000_0000, my_package.Countdown())`. Copy
[`modules/snn`](../modules/snn) (`Cargo.toml`, `pyproject.toml`,
`src/python.rs`) as a template.

### In C/C++

Fill a heap-allocated `mrve32_device` and wrap it in a capsule with
`PyCapsule_New(dev, MRVE32_DEVICE_CAPSULE_NAME, destructor)`. The
destructor frees the table and calls `dev->destroy(dev->ctx)` only if
`dev->ctx != NULL` (otherwise the emulator owns the device).

### Contract (ABI version 2)

- Callbacks are called from one thread at a time, never concurrently, and
  must not unwind (a Rust panic in an exported callback aborts).
- `destroy` is called exactly once.
- `read` writes `*value` only on `MRVE32_STATUS_OK`.
- The ABI version is bumped on any incompatible change; the emulator
  rejects modules built for another version with a clear error.

## Conventions for modules

- Name the package `mrve32-<device>` (Python module `mrve32_<device>`).
- Publish an `ID` register so drivers can check what is mapped where.
- Ship a host-side driver and, for compute devices, a reference model the
  device can be checked against (see `mrve32_snn.reference_step`).
- Document the register map; [the SNN spec](../modules/snn/docs/snn-accelerator-v0.md)
  is an example.
