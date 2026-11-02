"""SNN layer accelerator device module for the MRVE32 emulator.

Spec: ``docs/snn-accelerator-v0.md``.

- :class:`SnnAccelerator` is the device (Rust model). Attach it to an
  emulator like any device module::

      emu = mrve32.Emulator()
      emu.map_device(0x1000_0000, SnnAccelerator())

- :func:`pack_spikes` / :func:`unpack_spikes` convert between boolean arrays
  and the accelerator's packed spike vectors.
- :func:`reference_step` is the NumPy golden model of one timestep (spec §5).
  It must stay bit-exact with the Rust model.
- :class:`SnnDriver` drives an accelerator mapped into an emulator directly
  from Python (host-driven, no firmware needed).
"""

from __future__ import annotations

import numpy as np
import numpy.typing as npt
from mrve32 import Emulator

from ._native import (
    BIAS_BASE,
    CTRL_CLEAR_VMEM,
    CTRL_START,
    ID,
    IRQ_DONE,
    REG_CAPS0,
    REG_CAPS1,
    REG_CTRL,
    REG_CYCLES,
    REG_ID,
    REG_IN_SPIKES,
    REG_IRQ_EN,
    REG_IRQ_STATUS,
    REG_LAYER_SIZE,
    REG_NEURON_CFG,
    REG_OUT_SPIKES,
    REG_STATUS,
    SPIKE_IN_BASE,
    SPIKE_OUT_BASE,
    STATUS_BUSY,
    STATUS_DONE,
    STATUS_ERROR,
    VMEM_BASE,
    WEIGHTS_BASE,
    WINDOW_SIZE,
    SnnAccelerator,
    __version__,
)

ERR_INVALID_LAYER_SIZE = 1
ERR_WEIGHTS_DO_NOT_FIT = 2
ERR_INVALID_THRESHOLD = 3


def pack_spikes(spikes: npt.ArrayLike) -> npt.NDArray[np.uint32]:
    """Packs a boolean vector into 32-bit words (neuron k = bit k%32 of word k//32)."""
    bits = np.asarray(spikes, dtype=bool).ravel()
    padded = np.zeros(-(-bits.size // 32) * 32, dtype=bool)
    padded[: bits.size] = bits
    return np.packbits(padded, bitorder="little").view("<u4").astype(np.uint32)


def unpack_spikes(words: npt.ArrayLike, n: int) -> npt.NDArray[np.bool_]:
    """Unpacks the first ``n`` spikes from 32-bit words."""
    raw = np.asarray(words, dtype="<u4").view(np.uint8)
    return np.unpackbits(raw, bitorder="little")[:n].astype(bool)


def reference_step(
    spikes_in: npt.ArrayLike,
    weights: npt.ArrayLike,
    vmem: npt.ArrayLike,
    *,
    v_th: int,
    leak_shift: int = 0,
    bias: npt.ArrayLike | None = None,
) -> tuple[npt.NDArray[np.bool_], npt.NDArray[np.int16]]:
    """One timestep of one layer (spec §5).

    Args:
        spikes_in: bool[n_in] input spikes.
        weights: int8[n_in, n_out], ``weights[i, j]`` from input i to output j.
        vmem: int16[n_out] membrane potentials before the step.
        v_th: threshold, 1..32767.
        leak_shift: 0 (no leak) .. 15.
        bias: optional int16[n_out], added every step (``BIAS_EN``).

    Returns:
        (bool[n_out] output spikes, int16[n_out] potentials after the step).
    """
    spikes = np.asarray(spikes_in, dtype=bool)
    w = np.asarray(weights)
    v = np.asarray(vmem)
    if w.dtype != np.int8 or w.ndim != 2 or w.shape[0] != spikes.size:
        raise ValueError("weights must be int8 with shape (n_in, n_out)")
    if v.dtype != np.int16 or v.shape != (w.shape[1],):
        raise ValueError("vmem must be int16 with shape (n_out,)")
    if not 1 <= v_th <= 32767:
        raise ValueError("v_th must be in 1..32767")
    if not 0 <= leak_shift <= 15:
        raise ValueError("leak_shift must be in 0..15")

    current = np.zeros(w.shape[1], dtype=np.int64)
    if bias is not None:
        b = np.asarray(bias)
        if b.dtype != np.int16 or b.shape != v.shape:
            raise ValueError("bias must be int16 with shape (n_out,)")
        current += b
    current += w[spikes].astype(np.int64).sum(axis=0)

    v = v.astype(np.int64)
    if leak_shift:
        v = v - (v >> leak_shift)  # arithmetic shift (floor)
    v = np.clip(v + current, -32768, 32767)
    fired = v >= v_th
    v = np.where(fired, v - v_th, v)
    return fired, v.astype(np.int16)


class SnnError(RuntimeError):
    """The accelerator reported a configuration error (``STATUS.ERROR``)."""

    def __init__(self, code: int) -> None:
        names = {
            ERR_INVALID_LAYER_SIZE: "invalid layer size",
            ERR_WEIGHTS_DO_NOT_FIT: "weights do not fit",
            ERR_INVALID_THRESHOLD: "invalid threshold",
        }
        super().__init__(f"SNN accelerator error {code}: {names.get(code, 'unknown')}")
        self.code = code


class SnnDriver:
    """Host-side driver for an accelerator mapped at ``base`` in an emulator.

    Accesses go through the emulated bus, exactly as firmware would see them.
    """

    def __init__(self, emu: Emulator, base: int) -> None:
        self.emu = emu
        self.base = base
        if self._reg(REG_ID) >> 16 != ID >> 16:
            raise ValueError(f"no SNN accelerator at 0x{base:08X}")
        caps0 = self._reg(REG_CAPS0)
        self.max_inputs: int = caps0 & 0xFFFF
        self.max_outputs: int = caps0 >> 16
        self.weight_bytes: int = self._reg(REG_CAPS1)
        self.n_in = 0
        self.n_out = 0

    def _reg(self, offset: int) -> int:
        return self.emu.read_u32(self.base + offset)

    def _set_reg(self, offset: int, value: int) -> None:
        self.emu.write_u32(self.base + offset, value)

    # --- configuration ---------------------------------------------------

    def configure(
        self, n_in: int, n_out: int, *, v_th: int, leak_shift: int = 0, bias: bool = False
    ) -> None:
        """Writes LAYER_SIZE and NEURON_CFG (validated by the device on START)."""
        self._set_reg(REG_LAYER_SIZE, (n_out & 0xFFFF) << 16 | (n_in & 0xFFFF))
        cfg = (v_th & 0xFFFF) | (leak_shift & 0xF) << 16 | (1 << 24 if bias else 0)
        self._set_reg(REG_NEURON_CFG, cfg)
        self.n_in, self.n_out = n_in, n_out

    def load_layer(
        self,
        weights: npt.ArrayLike,
        *,
        v_th: int,
        leak_shift: int = 0,
        bias: npt.ArrayLike | None = None,
    ) -> None:
        """Writes weights (and bias) and configures the layer from their shape."""
        w = np.asarray(weights)
        if w.dtype != np.int8 or w.ndim != 2:
            raise ValueError("weights must be int8 with shape (n_in, n_out)")
        n_in, n_out = w.shape
        self.emu.write_bytes(self.base + WEIGHTS_BASE, np.ascontiguousarray(w).tobytes())
        if bias is not None:
            self.write_bias(bias)
        self.configure(n_in, n_out, v_th=v_th, leak_shift=leak_shift, bias=bias is not None)

    def write_bias(self, bias: npt.ArrayLike) -> None:
        b = np.asarray(bias, dtype=np.int16)
        self.emu.write_bytes(self.base + BIAS_BASE, b.astype("<i2").tobytes())

    def write_vmem(self, vmem: npt.ArrayLike) -> None:
        v = np.asarray(vmem, dtype=np.int16)
        self.emu.write_bytes(self.base + VMEM_BASE, v.astype("<i2").tobytes())

    def read_vmem(self, n: int | None = None) -> npt.NDArray[np.int16]:
        n = self.n_out if n is None else n
        raw = self.emu.read_bytes(self.base + VMEM_BASE, 2 * n)
        return np.frombuffer(raw, dtype="<i2").astype(np.int16)

    def write_input_spikes(self, spikes: npt.ArrayLike) -> None:
        words = pack_spikes(spikes)
        self.emu.write_bytes(self.base + SPIKE_IN_BASE, words.astype("<u4").tobytes())

    def read_output_spikes(self, n: int | None = None) -> npt.NDArray[np.bool_]:
        n = self.n_out if n is None else n
        raw = self.emu.read_bytes(self.base + SPIKE_OUT_BASE, 4 * -(-n // 32))
        return unpack_spikes(np.frombuffer(raw, dtype="<u4"), n)

    # --- control and status ----------------------------------------------

    @property
    def status(self) -> int:
        return self._reg(REG_STATUS)

    @property
    def busy(self) -> bool:
        return bool(self.status & STATUS_BUSY)

    @property
    def cycles(self) -> int:
        """Duration of the last completed step (CYCLES)."""
        return self._reg(REG_CYCLES)

    @property
    def in_spikes(self) -> int:
        return self._reg(REG_IN_SPIKES)

    @property
    def out_spikes(self) -> int:
        return self._reg(REG_OUT_SPIKES)

    def enable_irq(self, enabled: bool = True) -> None:
        self._set_reg(REG_IRQ_EN, IRQ_DONE if enabled else 0)

    def ack_irq(self) -> None:
        self._set_reg(REG_IRQ_STATUS, IRQ_DONE)

    def clear_vmem(self) -> None:
        self._set_reg(REG_CTRL, CTRL_CLEAR_VMEM)

    def start(self, *, clear_vmem: bool = False) -> None:
        self._set_reg(REG_CTRL, CTRL_START | (CTRL_CLEAR_VMEM if clear_vmem else 0))

    def wait(self, *, max_cycles: int = 10_000_000, chunk: int = 64) -> None:
        """Advances the devices (no CPU execution) until the step completes.

        Raises :class:`SnnError` if the step failed, ``TimeoutError`` if it
        did not finish within ``max_cycles``.
        """
        waited = 0
        while self.busy:
            if waited >= max_cycles:
                raise TimeoutError(f"SNN step still busy after {waited} cycles")
            self.emu.tick(chunk)
            waited += chunk
        status = self.status
        if status & STATUS_ERROR:
            raise SnnError(status >> 8 & 0xFF)

    def step(self, spikes_in: npt.ArrayLike, **wait_kwargs: int) -> npt.NDArray[np.bool_]:
        """Runs one timestep host-driven: write inputs, START, wait, read outputs."""
        self.write_input_spikes(spikes_in)
        self.start()
        self.wait(**wait_kwargs)
        self.ack_irq()
        return self.read_output_spikes()


__all__ = [
    "SnnAccelerator",
    "SnnDriver",
    "SnnError",
    "pack_spikes",
    "reference_step",
    "unpack_spikes",
    "__version__",
]
