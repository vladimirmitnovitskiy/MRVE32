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
