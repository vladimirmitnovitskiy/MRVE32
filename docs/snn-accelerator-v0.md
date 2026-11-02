# SNN Layer Accelerator — Specification v0 (draft)

Status: **draft**, subject to change until v1. Version reported by `ID`: `0.0`.

## 1. Overview

The SNN Layer Accelerator executes **one timestep of one fully connected
spiking layer** per command. The RISC-V core acts as the graph dispatcher: it
loads a layer's weights and state into the accelerator's local SRAM, starts a
step, waits for the completion interrupt, moves output spikes to the next
layer and repeats for every layer and timestep. There is no autonomous
network sequencer in v0.

Neurons are integrate-and-fire with optional leak (LIF/IF), signed 16-bit
membrane potentials with saturation, signed 8-bit weights, reset by
subtraction.

This document is normative for the **register interface** and the
**arithmetic** (results must be bit-exact across implementations). The
timing model in §6 is non-normative.

## 2. Memory map

All offsets are relative to the device base address (`SNN_BASE`, platform
defined). The device occupies a 1 MiB window. All data is little-endian.

| Offset range          | Region      | Contents                                  | Element |
|-----------------------|-------------|-------------------------------------------|---------|
| `0x0_0000 – 0x0_0FFF` | `REGS`      | control/status registers (§3)             | 32-bit  |
| `0x0_1000 – 0x0_17FF` | `SPIKE_IN`  | input spike bit vector                    | bit     |
| `0x0_1800 – 0x0_1FFF` | `SPIKE_OUT` | output spike bit vector                   | bit     |
| `0x0_2000 – 0x7_FFFF` | `WEIGHTS`   | weight matrix (§4.2)                      | `i8`    |
| `0x8_0000 – 0xB_FFFF` | `VMEM`      | membrane potential per output neuron      | `i16`   |
| `0xC_0000 – 0xF_FFFF` | `BIAS`      | bias per output neuron                    | `i16`   |

Each region is implemented only up to the instance's capacity
(`CAPS0`/`CAPS1`): `SPIKE_IN` = ⌈MAX_IN/32⌉ words, `SPIKE_OUT` =
⌈MAX_OUT/32⌉ words, `WEIGHTS` = `WEIGHT_BYTES` bytes, `VMEM` and `BIAS` =
MAX_OUT halfwords. Accesses beyond the implemented part of a region fail with
a bus error.

Upper bounds of the window: MAX_IN ≤ 16384, MAX_OUT ≤ 16384,
WEIGHT_BYTES ≤ 516096 (`0x7E000`).

### Access rules

- `REGS`: 32-bit aligned accesses only. Reserved register offsets read as 0
  and ignore writes.
- SRAM regions: 8/16/32-bit accesses, naturally aligned.
- Any other width or alignment fails with a bus error.
- While `STATUS.BUSY` is set, **every SRAM access fails with a bus error**.
  Registers remain accessible. (This makes firmware races visible in
  emulation instead of returning stale data.)

## 3. Registers

| Offset  | Name         | Access | Description |
|---------|--------------|--------|-------------|
| `0x000` | `CTRL`       | W, reads 0 | bit 0 `START`: run one timestep. bit 1 `CLEAR_VMEM`: zero all of `VMEM`. If both are set, the clear happens first. Ignored while BUSY. |
| `0x004` | `STATUS`     | RO     | bit 0 `BUSY`, bit 1 `DONE`, bit 2 `ERROR`, bits [15:8] `ERR_CODE` (§3.1). `DONE` is cleared by `START` and set when the step completes or fails. |
| `0x008` | `IRQ_EN`     | RW     | bit 0 `DONE_IE`. |
| `0x00C` | `IRQ_STATUS` | RW1C   | bit 0 `DONE_IP`: set together with `STATUS.DONE`; write 1 to clear. |
| `0x010` | `LAYER_SIZE` | RW     | bits [15:0] `N_IN`, bits [31:16] `N_OUT`. |
| `0x014` | `NEURON_CFG` | RW     | bits [15:0] `V_TH` (`i16`), bits [19:16] `LEAK_SHIFT` (0 = no leak), bit 24 `BIAS_EN`. Other bits read 0. |
| `0x018` | `CYCLES`     | RO     | duration of the last completed step, in clock cycles (§6). |
| `0x01C` | `IN_SPIKES`  | RO     | number of input spikes among the first `N_IN` bits in the last completed step. |
| `0x020` | `OUT_SPIKES` | RO     | number of output spikes produced by the last completed step. |
| `0x0F0` | `ID`         | RO     | `0x534E_MMmm`: ASCII `"SN"`, major `MM`, minor `mm`. v0 = `0x534E_0000`. |
| `0x0F4` | `CAPS0`      | RO     | bits [15:0] `MAX_IN`, bits [31:16] `MAX_OUT`. |
| `0x0F8` | `CAPS1`      | RO     | `WEIGHT_BYTES`. |

`LAYER_SIZE` and `NEURON_CFG` writes are ignored while BUSY; their values
are latched by `START`.

The interrupt output is asserted while `IRQ_EN.DONE_IE & IRQ_STATUS.DONE_IP`.
On the reference platform it drives the hart's machine external interrupt
(`mip.MEIP`).

Reset state: all registers 0 (except `ID`, `CAPS0`, `CAPS1`), all SRAM 0.

### 3.1 Errors

`START` validates the latched configuration. On failure no SRAM is modified,
`STATUS` = `DONE | ERROR | ERR_CODE << 8`, `DONE_IP` is set immediately and
`CYCLES`/`IN_SPIKES`/`OUT_SPIKES` are 0.

| `ERR_CODE` | Condition |
|------------|-----------|
| 1 | `N_IN` = 0, `N_OUT` = 0, `N_IN` > `MAX_IN` or `N_OUT` > `MAX_OUT` |
| 2 | `N_IN × N_OUT` > `WEIGHT_BYTES` |
| 3 | `V_TH` ≤ 0 |

## 4. Data formats

### 4.1 Spike vectors

Neuron *k* is bit `k % 32` of 32-bit word `k / 32` (LSB first). In
`SPIKE_IN`, bits ≥ `N_IN` are ignored. A step writes words
`0 … ⌈N_OUT/32⌉−1` of `SPIKE_OUT` completely (bits ≥ `N_OUT` are written 0);
later words are not modified.

### 4.2 Weights

Input-major, row per input neuron: the weight from input *i* to output *j* is
the `i8` at `WEIGHTS + i × N_OUT + j`. An input spike therefore selects one
contiguous row (event-driven fan-out).

### 4.3 Membrane potentials and bias

`VMEM[j]` and `BIAS[j]` are `i16` at `VMEM + 2j` and `BIAS + 2j`. `VMEM`
holds the layer state between timesteps; firmware saves and restores it when
time-multiplexing several layers.

## 5. Step semantics (normative)

```
for j in 0 .. N_OUT:
    I[j] = BIAS_EN ? BIAS[j] : 0                  # i32
for i in 0 .. N_IN where SPIKE_IN bit i is set:
    for j in 0 .. N_OUT:
        I[j] += W[i][j]                           # i32, cannot overflow
for j in 0 .. N_OUT:
    v = VMEM[j]                                   # i16
    if LEAK_SHIFT != 0:
        v = v - (v >> LEAK_SHIFT)                 # arithmetic shift, cannot overflow
    v = clamp(v + I[j], -32768, 32767)
    if v >= V_TH:
        SPIKE_OUT bit j = 1
        v = v - V_TH                              # reset by subtraction, V_TH > 0
    else:
        SPIKE_OUT bit j = 0
    VMEM[j] = v
```

Notes:

- Synaptic input is accumulated at full precision (`i32`) and saturated once
  per neuron, so the result does not depend on the order in which an
  implementation visits spikes or neurons.
- A neuron emits at most one spike per step; potential above `V_TH` is kept
  as residual (reset by subtraction).
- `LEAK_SHIFT = 0` gives a non-leaky IF neuron, which is what CNN→SNN
  rate-coded conversion usually uses.

## 6. Timing model (non-normative)

The emulator model sets `BUSY` for

```
cycles = T_SETUP + N_OUT × T_NEURON + IN_SPIKES × ⌈N_OUT / LANES⌉ × T_SYN
```

clock cycles after `START`, with defaults `T_SETUP = 8`, `T_NEURON = 1`,
`T_SYN = 1`, `LANES = 8`. The parameters are configurable per instance and
are estimates for design-space exploration, not a property of the interface.

## 7. Programming sequence

```
write WEIGHTS, BIAS                          # once per layer
write LAYER_SIZE, NEURON_CFG                 # per layer
CTRL = CLEAR_VMEM                            # at the start of an inference
for t in 0 .. T:
    write SPIKE_IN
    IRQ_EN = DONE_IE; CTRL = START
    wfi until DONE (interrupt or STATUS poll)
    IRQ_STATUS = DONE_IP                     # acknowledge
    read SPIKE_OUT
```

## 8. Open questions for v1

- DMA so the accelerator can fetch weights/spikes from system RAM.
- Convolutional layers (weight sharing) and pooling.
- Multiple spikes per step / graded spikes.
- Per-layer weight scale (shift) before accumulation.
- Batch of timesteps per command.
