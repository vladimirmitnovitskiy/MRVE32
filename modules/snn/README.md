[![PyPI](https://img.shields.io/pypi/v/mrve32-snn.svg)](https://pypi.org/project/mrve32-snn/)
[![Python](https://img.shields.io/pypi/pyversions/mrve32-snn.svg)](https://pypi.org/project/mrve32-snn/)
[![CI](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml/badge.svg)](https://github.com/vladimirmitnovitskiy/MRVE32/actions/workflows/ci.yml)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/modules/snn/LICENSE)

# mrve32-snn

SNN layer accelerator **device module** for the [MRVE32](https://github.com/vladimirmitnovitskiy/MRVE32) RISC-V emulator:
a behavioural (golden) model of a memory-mapped spiking-layer accelerator,
spec [v0 draft](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/modules/snn/docs/snn-accelerator-v0.md).

- LIF/IF neurons, `i16` saturating potentials, `i8` weights, reset by subtraction
- one layer per START, local SRAM (spikes, weights, potentials, bias)
- analytical cycle model, completion interrupt (MEIP)
- the computation sits behind a swappable `SnnBackend` (default: `ReferenceLif`)

It plugs into the emulator through the public device interfaces only
(`BusDevice` in Rust, the device C ABI in Python), like any third-party device;
see [`docs/devices.md`](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/devices.md).

## Install

```bash
pip install mrve32-snn        # also installs mrve32 and NumPy
```

Wheels exist for Linux, macOS and Windows, CPython 3.10 and newer. The
emulator's own Python API is documented in
[docs/python.md](https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/python.md).

## Python

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

## Rust

```rust
use mrve32::Bus;
use mrve32_snn::{SNN_WINDOW_SIZE, SnnAccelerator, SnnConfig};

let mut bus = Bus::new();
bus.map_device(0x1000_0000, SNN_WINDOW_SIZE, Box::new(SnnAccelerator::new(SnnConfig::default())))
    .unwrap();
```
