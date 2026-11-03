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

## Rust

```rust
use mrve32::Bus;
use mrve32_snn::{SNN_WINDOW_SIZE, SnnAccelerator, SnnConfig};

let mut bus = Bus::new();
bus.map_device(0x1000_0000, SNN_WINDOW_SIZE, Box::new(SnnAccelerator::new(SnnConfig::default())))
    .unwrap();
```

## Python

```bash
pip install ../..          # mrve32 core first (not on PyPI yet)
pip install .
```

```python
from mrve32 import Emulator
from mrve32_snn import SnnAccelerator, SnnDriver, reference_step, pack_spikes

emu = Emulator()
emu.map_device(0x1000_0000, SnnAccelerator(max_inputs=1024, max_outputs=512))
dev = SnnDriver(emu, 0x1000_0000)   # host-side driver over the emulated bus
```

`reference_step` is the NumPy golden model of one timestep; the tests check
the Rust model against it bit-exactly.
