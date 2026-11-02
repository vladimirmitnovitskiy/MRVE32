"""Pluggable devices: native modules (C ABI capsule) and Python objects."""

import pytest

import mrve32
from mrve32 import BusError, Clint, Emulator, Ram

WFI = 0x1050_0073
NOP = 0x0000_0013
DEV = 0x4000_0000


class Register:
    """A 4-byte read/write register with a cycle counter and an IRQ line."""

    def __init__(self):
        self.value = 0
        self.ticks = 0
        self.log = []

    def read(self, offset, width):
        self.log.append(("r", offset, width))
        if offset == 4:
            return self.ticks & ((1 << (8 * width)) - 1)
        if offset != 0:
            raise BusError("no register here")
        return self.value & ((1 << (8 * width)) - 1)

    def write(self, offset, width, value):
        self.log.append(("w", offset, width, value))
        self.value = value

    def tick(self, cycles):
        self.ticks += cycles

    def irq_lines(self):
        return 1 << 11 if self.value == 0xA5 else 0  # MEIP


def test_native_devices_use_the_plugin_abi():
    emu = Emulator(clint_base=None)
    assert [name for *_, name in emu.devices] == ["Ram"]
    emu.map_device(0x2000_0000, Ram(256))
    emu.map_device(0x0200_0000, Clint(divider=2))
    assert emu.devices == [
        (0x0200_0000, 0x1_0000, "Clint"),
        (0x2000_0000, 256, "Ram"),
        (0x8000_0000, 64 * 1024, "Ram"),
    ]
    emu.write_u32(0x2000_00FC, 0xCAFEBABE)
    assert emu.read_u16(0x2000_00FE) == 0xCAFE
    with pytest.raises(BusError):
        emu.read_u8(0x2000_0100)
    emu.tick(10)
    assert emu.read_u32(0x0200_BFF8) == 5  # mtime with divider 2


def test_native_device_errors():
    emu = Emulator()
    ram = Ram(16)
    emu.map_device(DEV, ram)
    with pytest.raises(RuntimeError, match="already attached"):
        emu.map_device(DEV + 0x100, ram)
    with pytest.raises(ValueError, match="overlaps"):
        emu.map_device(0x8000_0010, Ram(16))
    with pytest.raises(TypeError, match="size cannot be given"):
        emu.map_device(DEV + 0x100, Ram(16), size=16)
    with pytest.raises(ValueError, match="32-bit address space"):
        emu.map_device(0xFFFF_FFF0, Ram(32))

    class Fake:
        def __mrve32_device__(self):
            return object()

    with pytest.raises(TypeError, match="capsule"):
        emu.map_device(DEV + 0x100, Fake())


def test_python_device_host_access():
    emu = Emulator()
    reg = Register()
    emu.map_device(DEV, reg, size=8)
    emu.write_u16(DEV, 0x1234)
    assert reg.value == 0x1234
    assert emu.read_u8(DEV) == 0x34
    assert reg.log == [("w", 0, 2, 0x1234), ("r", 0, 1)]
    with pytest.raises(BusError):
        emu.read_u32(DEV + 2)  # device raised BusError
    assert isinstance(reg, mrve32.Device)


def test_python_device_from_firmware_with_interrupt():
    emu = Emulator()
    reg = Register()
    emu.map_device(DEV, reg, size=8)
    # lui a0, 0x40000; li a1, 0xA5; sw a1, 0(a0); wfi; lw a2, 4(a0)
    emu.load_program(
        emu.ram_base, [0x40000537, 0x0A500593, 0x00B52023, WFI, 0x00452603]
    )
    emu.write_csr(0x304, 1 << 11)  # mie.MEIE (mstatus.MIE = 0: wake, no trap)
    res = emu.run(5)
    assert res.reason == "max_steps"
    assert reg.value == 0xA5
    assert emu.read_reg(12) == reg.ticks - 1 == 4  # tick() runs after each step
    assert emu.pc == emu.ram_base + 20


def test_python_device_size_attribute_and_validation():
    emu = Emulator()

    class Sized(Register):
        size = 16

    emu.map_device(DEV, Sized())
    assert (DEV, 16, "test_python_device_size_attribute_and_validation.<locals>.Sized") in emu.devices
    with pytest.raises(TypeError, match="size"):
        emu.map_device(DEV + 0x100, Register())
    with pytest.raises(TypeError, match="not a device"):
        emu.map_device(DEV + 0x100, object(), size=4)


def test_exception_in_python_device_propagates_and_stops_run():
    class Broken:
        size = 4

        def read(self, offset, width):
            raise ZeroDivisionError("model bug")

        def write(self, offset, width, value):
            pass

    emu = Emulator()
    emu.map_device(DEV, Broken())
    with pytest.raises(ZeroDivisionError, match="model bug"):
        emu.read_u32(DEV)

    # lui a0, 0x40000; lw a1, 0(a0); then NOPs
    emu.load_program(emu.ram_base, [0x40000537, 0x00052583] + [NOP] * 8)
    with pytest.raises(ZeroDivisionError):
        emu.run(1000)
    assert emu.cycles == 2, "the run stops at the faulting step"


def test_read_value_must_fit_width():
    class Wide(Register):
        def read(self, offset, width):
            return 0x1FF

    emu = Emulator()
    emu.map_device(DEV, Wide(), size=4)
    with pytest.raises(ValueError, match="does not fit"):
        emu.read_u8(DEV)
    assert emu.read_u32(DEV) == 0x1FF


FIRMWARE = __import__("pathlib").Path(__file__).resolve().parents[1] / "firmware" / "hello.elf"


def virt(input=b""):
    """QEMU-virt-like machine, as used by the mrve32 runner."""
    emu = Emulator(ram_size=1 << 20)
    uart = mrve32.Uart()
    emu.map_device(0x1000_0000, uart)
    emu.map_device(0x0010_0000, mrve32.TestFinisher())
    uart.write_input(input)
    emu.load_elf(FIRMWARE)
    return emu, uart


def test_c_firmware_with_uart_and_finisher():
    emu, uart = virt()
    res = emu.run(100_000)
    assert (res.reason, res.exit_code, res.trap) == ("exit", 0, None)
    assert uart.output() == b"Hello from MRVE32!\nfib(20) = 6765\n"
    assert uart.take_output().startswith(b"Hello")
    assert uart.output() == b""
    assert "exit_code=0" in repr(res)

    emu, uart = virt(b"fail")
    res = emu.run(100_000)
    assert (res.reason, res.exit_code) == ("exit", 3)
    assert uart.output().endswith(b"input: fail")
    assert uart.pending_input == 0


def test_uart_echo(capsys):
    emu = Emulator()
    uart = mrve32.Uart(echo=True)
    emu.map_device(0x1000_0000, uart)
    for b in b"one\ntwo":
        emu.write_u8(0x1000_0000, b)
    assert capsys.readouterr().out == "one\n", "line buffered"
    assert uart.output() == b""
    with pytest.raises(ValueError):
        mrve32.Uart(reg_shift=3)


def test_python_device_exit_request():
    class Halt:
        size = 4
        code = None

        def read(self, offset, width):
            return 0

        def write(self, offset, width, value):
            self.code = value

        def exit_request(self):
            return self.code

    emu = Emulator()
    emu.map_device(DEV, Halt())
    # lui a0, 0x40000; li a1, 9; sw a1, 0(a0); nop
    emu.load_program(emu.ram_base, [0x40000537, 0x00900593, 0x00B52023, NOP])
    res = emu.run(10)
    assert (res.reason, res.exit_code, res.steps) == ("exit", 9, 3)


def test_tracer_and_disassemble():
    emu, _ = virt()
    trace = []
    emu.set_tracer(lambda pc, word, trap: trace.append((pc, word, trap)))
    emu.run(3)
    assert [pc for pc, *_ in trace] == [0x8000_0000, 0x8000_0004, 0x8000_0008]
    pc, word, trap = trace[0]
    assert mrve32.disassemble(word, pc) == "auipc sp, 0x100"
    assert trap is None
    assert mrve32.disassemble(0x0080006F) == "j 8"
    assert mrve32.disassemble(0xFFFF_FFFF) == ".word 0xffffffff"

    emu.set_tracer(None)
    emu.run(3)
    assert len(trace) == 3

    def broken(pc, word, trap):
        raise RuntimeError("tracer bug")

    emu.set_tracer(broken)
    with pytest.raises(RuntimeError, match="tracer bug"):
        emu.run(100)
    assert emu.cycles == 7, "the run stops at the first failing callback"


def _capsule(name, abi_version):
    """A capsule holding a device table with the given ABI version; the
    emulator must reject it before calling anything in it."""
    import ctypes

    class Table(ctypes.Structure):
        _fields_ = [("abi_version", ctypes.c_uint32), ("size", ctypes.c_uint32),
                    ("ctx", ctypes.c_void_p)] + [(f, ctypes.c_void_p) for f in
                    ("read", "write", "tick", "irq_lines", "destroy", "exit_request")]

    table = Table(abi_version=abi_version, size=4, ctx=1)
    new = ctypes.pythonapi.PyCapsule_New
    new.restype = ctypes.py_object
    new.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_void_p]
    capsule = new(ctypes.addressof(table), name, None)
    return capsule, table  # keep the table alive


def test_abi_mismatch_is_a_clean_error():
    emu = Emulator()

    class Device:
        def __init__(self, capsule):
            self.capsule = capsule

        def __mrve32_device__(self):
            return self.capsule

    old_name, t1 = _capsule(b"mrve32.device.v1", 1)
    with pytest.raises(ValueError, match="older mrve32 device ABI"):
        emu.map_device(DEV, Device(old_name))

    other_version, t2 = _capsule(b"mrve32.device", mrve32.DEVICE_ABI_VERSION + 1)
    with pytest.raises(ValueError, match="ABI version .* is not supported"):
        emu.map_device(DEV, Device(other_version))

    foreign, t3 = _capsule(b"something.else", mrve32.DEVICE_ABI_VERSION)
    with pytest.raises(TypeError, match="capsule"):
        emu.map_device(DEV, Device(foreign))
    assert emu.devices == [(0x0200_0000, 0x1_0000, "Clint"), (0x8000_0000, 0x1_0000, "Ram")]
