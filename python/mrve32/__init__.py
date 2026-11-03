"""MRVE32: deterministic RV32IMAC microcontroller emulator.

The core (CPU, bus, RAM, CLINT, ELF loader) is implemented in Rust
(``mrve32._native``). Peripherals are pluggable: ``Emulator.map_device``
accepts native device modules (objects with ``__mrve32_device__()``, built
against the C ABI in ``include/mrve32_device.h``) and plain Python objects
with ``read``/``write`` methods.

Documentation: https://github.com/vladimirmitnovitskiy/MRVE32/blob/main/docs/python.md
"""

from typing import Protocol, runtime_checkable

from ._native import (
    DEVICE_ABI_VERSION,
    BusError,
    Clint,
    CoprocessorCall,
    ElfError,
    Emulator,
    Ram,
    RunResult,
    TestFinisher,
    Trap,
    Uart,
    __version__,
    disassemble,
)


@runtime_checkable
class Device(Protocol):
    """Interface of a device implemented in Python.

    ``tick(cycles)``, ``irq_lines() -> int`` and
    ``exit_request() -> int | None`` are optional. Raise :class:`BusError`
    from ``read``/``write`` to signal an access fault.
    """

    def read(self, offset: int, width: int) -> int: ...
    def write(self, offset: int, width: int, value: int) -> None: ...


@runtime_checkable
class Coprocessor(Protocol):
    """Interface of a coprocessor for custom-0/custom-1 instructions
    (see ``Emulator.set_coprocessor``).

    Return ``False`` from ``execute`` for an illegal instruction; ``None``
    or ``True`` means it executed.
    """

    def execute(self, call: CoprocessorCall) -> bool | None: ...


__all__ = [
    "DEVICE_ABI_VERSION",
    "BusError",
    "Clint",
    "Coprocessor",
    "CoprocessorCall",
    "Device",
    "ElfError",
    "Emulator",
    "Ram",
    "RunResult",
    "TestFinisher",
    "Trap",
    "Uart",
    "__version__",
    "disassemble",
]
