from collections.abc import Callable
from os import PathLike
from typing import Final, Literal, Protocol

__version__: Final[str]

class BusError(Exception):
    """A bus access failed (unmapped address, invalid width/alignment, device busy)."""

class ElfError(Exception):
    """An ELF file could not be parsed or loaded."""

class Trap:
    """A trap taken by the CPU; already handled, ``pc`` points to the trap vector."""

    @property
    def cause(self) -> int:
        """``mcause`` (bit 31 set for interrupts)."""
    @property
    def is_interrupt(self) -> bool: ...
    @property
    def tval(self) -> int:
        """``mtval``."""
    @property
    def epc(self) -> int:
        """``mepc``."""
    @property
    def description(self) -> str: ...

class RunResult:
    @property
    def steps(self) -> int:
        """Clock cycles executed by the call."""
    @property
    def reason(self) -> Literal["max_steps", "breakpoint", "trap", "idle", "exit"]: ...
    @property
    def trap(self) -> Trap | None:
        """The trap that stopped the run when ``reason == "trap"``."""
    @property
    def exit_code(self) -> int | None:
        """The code requested by a device (e.g. TestFinisher) when ``reason == "exit"``."""

class Ram:
    """Main memory; attach with ``Emulator.map_device(base, Ram(size))``."""

    def __init__(self, size: int) -> None: ...
    @property
    def size(self) -> int: ...
    def __mrve32_device__(self) -> object: ...

class Clint:
    """Core-local interruptor (msip, mtime, mtimecmp; SiFive map, 64 KiB window)."""

    def __init__(self, divider: int = 1) -> None:
        """``mtime`` advances by one every ``divider`` CPU cycles."""
    @property
    def size(self) -> int: ...
    def __mrve32_device__(self) -> object: ...

class Uart:
    """NS16550A-compatible UART (QEMU ``virt`` register model).

    Transmitted bytes are collected, or with ``echo=True`` written line by
    line to ``sys.stdout``. The object stays usable after it is attached.
    """

    def __init__(self, *, reg_shift: int = 0, echo: bool = False) -> None:
        """``reg_shift``: registers ``1 << reg_shift`` bytes apart (0..2)."""
    @property
    def size(self) -> int: ...
    def output(self) -> bytes:
        """Bytes transmitted so far (not cleared)."""
    def take_output(self) -> bytes:
        """Bytes transmitted since the last call; clears the buffer."""
    def write_input(self, data: bytes | bytearray) -> None:
        """Queues bytes for the firmware to receive."""
    @property
    def pending_input(self) -> int: ...
    def __mrve32_device__(self) -> object: ...

class TestFinisher:
    """``sifive,test0``: firmware writes ``0x5555`` (pass) or
    ``(code << 16) | 0x3333`` (fail); ``run()`` then returns
    ``reason == "exit"``. 4 KiB window."""

    def __init__(self) -> None: ...
    @property
    def size(self) -> int: ...
    def __mrve32_device__(self) -> object: ...

def disassemble(word: int, pc: int | None = None) -> str:
    """One instruction in GNU objdump style; absolute targets with ``pc``.

    ``word`` is a 32-bit instruction, or a 16-bit (C) one in the low half
    when its low two bits are not ``0b11``."""

class CoprocessorCall:
    """One custom instruction handed to a coprocessor's ``execute``.

    Valid only during that call; afterwards every method raises
    ``RuntimeError``. Register and ``next_pc`` changes are committed only if
    ``execute`` succeeds.
    """

    @property
    def opcode(self) -> int:
        """``0x0B`` (custom-0) or ``0x2B`` (custom-1)."""
    @property
    def word(self) -> int:
        """The 32-bit instruction word."""
    @property
    def pc(self) -> int: ...
    @property
    def rd(self) -> int: ...
    @property
    def rs1(self) -> int: ...
    @property
    def rs2(self) -> int: ...
    @property
    def funct3(self) -> int: ...
    @property
    def funct7(self) -> int: ...
    next_pc: int
    """The pc after the instruction (``pc + 4`` by default); set it to branch."""
    def read_reg(self, reg: int) -> int: ...
    def write_reg(self, reg: int, value: int) -> None:
        """Writes to x0 are discarded."""
    # Bus access; failures raise BusError (an access fault if uncaught).
    def read_u8(self, addr: int) -> int: ...
    def read_u16(self, addr: int) -> int: ...
    def read_u32(self, addr: int) -> int: ...
    def write_u8(self, addr: int, value: int) -> None: ...
    def write_u16(self, addr: int, value: int) -> None: ...
    def write_u32(self, addr: int, value: int) -> None: ...

class _Coprocessor(Protocol):
    def execute(self, call: CoprocessorCall) -> bool | None:
        """Executes ``call``; return ``False`` for an illegal instruction."""

class Emulator:
    """A RISC-V MCU: CPU and bus with RAM and an optional CLINT.

    Further devices are attached with :meth:`map_device`. The CPU starts at
    ``ram_base`` (or at the ELF entry after ``load_elf``).
    """

    def __init__(
        self,
        ram_base: int = 0x8000_0000,
        ram_size: int = 64 * 1024,
        *,
        clint_base: int | None = 0x0200_0000,
        compressed: bool = True,
    ) -> None: ...
    @property
    def ram_base(self) -> int: ...
    @property
    def clint_base(self) -> int | None: ...
    compressed: bool
    """C extension enabled (RV32IMAC); ``False`` models an RV32IMA core."""

    # Devices
    def map_device(self, base: int, device: object, *, size: int | None = None) -> None:
        """Maps a device at ``base``.

        ``device`` is a native device module (has ``__mrve32_device__()``;
        its window size is fixed) or a Python object implementing
        :class:`mrve32.Device` with a window of ``size`` or ``device.size``
        bytes. Raises ``ValueError`` if the window overlaps another device.
        """
    def set_tracer(self, tracer: Callable[[int, int | None, Trap | None], object] | None) -> None:
        """Calls ``tracer(pc, word, trap)`` after every step; ``None`` removes it."""
    def set_coprocessor(self, coprocessor: _Coprocessor | None) -> None:
        """Handles custom-0/custom-1 instructions (opcodes 0x0B, 0x2B) with
        ``coprocessor.execute(call)``; ``None`` detaches it.

        Returning ``False`` raises an illegal-instruction trap. An uncaught
        ``BusError`` from a ``call`` access becomes a load/store access
        fault; any other exception stops :meth:`run` and is re-raised.
        """
    @property
    def devices(self) -> list[tuple[int, int, str]]:
        """Mapped devices as ``(base, size, type name)``, sorted by address."""

    # Execution
    def step(self) -> Trap | None:
        """Executes one clock cycle; returns the trap taken, if any."""
    def run(self, max_steps: int, *, stop_on_trap: bool = False) -> RunResult:
        """Runs up to ``max_steps`` cycles, stopping at breakpoints, on traps
        (if ``stop_on_trap``) or when the hart can never wake from ``wfi``.

        An exception raised by a Python device stops the run and propagates."""
    def tick(self, cycles: int) -> None:
        """Advances the devices without executing instructions."""
    def add_breakpoint(self, pc: int) -> None: ...
    def remove_breakpoint(self, pc: int) -> bool: ...
    def clear_breakpoints(self) -> None: ...
    @property
    def breakpoints(self) -> list[int]: ...

    # Loading
    def load_elf(self, source: str | PathLike[str] | bytes | bytearray) -> int:
        """Loads an ELF (path or bytes), sets ``pc`` to its entry and returns it."""
    def symbol(self, name: str) -> int | None: ...
    @property
    def symbols(self) -> dict[str, int]: ...
    def load_program(self, addr: int, program: list[int]) -> None:
        """Writes 32-bit words starting at ``addr``."""

    # Memory (raise BusError on failure)
    def read_bytes(self, addr: int, size: int) -> bytes: ...
    def write_bytes(self, addr: int, data: bytes | bytearray) -> None: ...
    def read_u8(self, addr: int) -> int: ...
    def read_u16(self, addr: int) -> int: ...
    def read_u32(self, addr: int) -> int: ...
    def write_u8(self, addr: int, val: int) -> None: ...
    def write_u16(self, addr: int, val: int) -> None: ...
    def write_u32(self, addr: int, val: int) -> None: ...

    # CPU state
    def read_reg(self, reg: int) -> int: ...
    def write_reg(self, reg: int, val: int) -> None: ...
    @property
    def regs(self) -> list[int]:
        """Snapshot of x0..x31."""
    pc: int
    def read_csr(self, csr: int) -> int: ...
    def write_csr(self, csr: int, val: int) -> None: ...
    @property
    def cycles(self) -> int:
        """64-bit ``mcycle``."""
    @property
    def instret(self) -> int:
        """64-bit ``minstret``."""
    @property
    def is_waiting(self) -> bool:
        """True while the hart waits in ``wfi``."""

DEVICE_ABI_VERSION: Final[int]
