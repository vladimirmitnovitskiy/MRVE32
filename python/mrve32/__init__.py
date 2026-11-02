"""MRVE32: deterministic RV32IM microcontroller emulator.

The core (CPU, bus, RAM, CLINT, ELF loader) is implemented in Rust
(``mrve32._native``). Peripherals are pluggable: ``Emulator.map_device``
accepts native device modules (objects with ``__mrve32_device__()``, built
against the C ABI in ``include/mrve32_device.h``) and plain Python objects
with ``read``/``write`` methods. See ``docs/devices.md``.
"""

from typing import Protocol, runtime_checkable

from ._native import (
    DEVICE_ABI_VERSION,
    BusError,
    Clint,
    ElfError,
    Emulator,
    Ram,
    RunResult,
    Trap,
    __version__,
)


@runtime_checkable
class Device(Protocol):
    """Interface of a device implemented in Python.

    ``tick(cycles)`` and ``irq_lines() -> int`` are optional. Raise
    :class:`BusError` from ``read``/``write`` to signal an access fault.
    """

    def read(self, offset: int, width: int) -> int: ...
    def write(self, offset: int, width: int, value: int) -> None: ...


__all__ = [
    "DEVICE_ABI_VERSION",
    "BusError",
    "Clint",
    "Device",
    "ElfError",
    "Emulator",
    "Ram",
    "RunResult",
    "Trap",
    "__version__",
]
