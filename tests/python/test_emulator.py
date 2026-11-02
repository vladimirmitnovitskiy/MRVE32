from pathlib import Path

import pytest

import mrve32
from mrve32 import BusError, ElfError, Emulator

ISA = Path(__file__).resolve().parents[1] / "isa"
NOP = 0x0000_0013
WFI = 0x1050_0073
MTVEC, MIE, MSTATUS = 0x305, 0x304, 0x300


def test_version_and_repr():
    assert mrve32.__version__
    assert repr(Emulator()).endswith("devices=[Clint@0x02000000, Ram@0x80000000])")


def test_executes_program():
    emu = Emulator()
    # addi x1, x0, 15; addi x2, x0, 27; add x3, x1, x2
    emu.load_program(emu.ram_base, [0x00F00093, 0x01B00113, 0x002081B3])
    for _ in range(3):
        assert emu.step() is None
    assert emu.read_reg(3) == 42
    assert emu.regs[1:4] == [15, 27, 42]
    assert emu.pc == emu.ram_base + 12
    assert emu.cycles == 3 and emu.instret == 3


def test_trap_object():
    emu = Emulator()
    emu.load_program(emu.ram_base, [0xFFFFFFFF])
    emu.write_csr(MTVEC, emu.ram_base + 0x100)
    trap = emu.step()
    assert trap is not None
    assert (trap.cause, trap.is_interrupt, trap.tval, trap.epc) == (
        2,
        False,
        0xFFFFFFFF,
        emu.ram_base,
    )
    assert "illegal instruction" in trap.description
    assert emu.pc == emu.ram_base + 0x100


def test_compressed_instructions_and_rv32im_mode():
    emu = Emulator()
    assert emu.compressed and emu.read_csr(0x301) & (1 << 2), "misa.C"
    # c.li a0, 5; c.addi a0, -1; addi a1, a0, 1 (32-bit, 2-byte aligned)
    emu.write_bytes(emu.ram_base, bytes.fromhex("1545" "7d15" "9305" "1500"))
    for _ in range(3):
        assert emu.step() is None
    assert (emu.read_reg(10), emu.read_reg(11), emu.pc) == (4, 5, emu.ram_base + 8)

    emu = Emulator(compressed=False)
    assert not emu.compressed and not emu.read_csr(0x301) & (1 << 2)
    emu.write_csr(MTVEC, emu.ram_base + 0x100)
    emu.load_program(emu.ram_base, [0x0060_00EF])  # jal ra, 6: misaligned without C
    trap = emu.step()
    assert (trap.cause, trap.tval) == (0, emu.ram_base + 6)


def test_errors():
    emu = Emulator()
    with pytest.raises(IndexError):
        emu.read_reg(32)
    with pytest.raises(IndexError):
        emu.write_reg(100, 1)
    with pytest.raises(BusError, match="not mapped"):
        emu.read_u32(0x1234_0000)
    with pytest.raises(ValueError, match="not implemented"):
        emu.read_csr(0x180)  # satp
    with pytest.raises(ValueError, match="overlaps"):
        Emulator(ram_base=0x0200_0000)  # RAM over the CLINT


def test_bulk_memory_access():
    emu = Emulator()
    data = bytes(range(256)) * 4
    emu.write_bytes(emu.ram_base + 3, data)
    assert emu.read_bytes(emu.ram_base + 3, len(data)) == data
    emu.write_bytes(emu.ram_base, bytearray(b"\x78\x56\x34\x12"))
    assert emu.read_u32(emu.ram_base) == 0x12345678
    assert emu.read_u16(emu.ram_base + 2) == 0x1234


def test_run_breakpoints_and_idle():
    emu = Emulator()
    emu.load_program(emu.ram_base, [NOP] * 8 + [WFI])

    res = emu.run(5)
    assert (res.steps, res.reason, res.trap) == (5, "max_steps", None)

    emu.add_breakpoint(emu.ram_base + 0x18)
    assert emu.breakpoints == [emu.ram_base + 0x18]
    res = emu.run(100)
    assert (res.steps, res.reason) == (1, "breakpoint")
    assert emu.pc == emu.ram_base + 0x18

    emu.clear_breakpoints()
    res = emu.run(100)
    assert res.reason == "idle", "wfi with mie == 0 never wakes"
    assert emu.is_waiting


def test_run_stop_on_trap():
    emu = Emulator()
    emu.load_program(emu.ram_base, [NOP, 0x00000073])  # nop; ecall
    emu.write_csr(MTVEC, emu.ram_base + 0x40)
    res = emu.run(10, stop_on_trap=True)
    assert res.reason == "trap" and res.steps == 2
    assert res.trap.cause == 11 and res.trap.epc == emu.ram_base + 4


def test_timer_interrupt_through_clint():
    emu = Emulator()
    emu.load_program(emu.ram_base, [WFI, NOP])
    emu.write_u32(emu.clint_base + 0x4000, 5)  # mtimecmp = 5
    emu.write_u32(emu.clint_base + 0x4004, 0)
    emu.write_csr(MTVEC, emu.ram_base + 0x80)
    emu.write_csr(MIE, 1 << 7)  # MTIE
    emu.write_csr(MSTATUS, 1 << 3)  # MIE

    res = emu.run(100, stop_on_trap=True)
    assert res.reason == "trap"
    assert res.trap.is_interrupt and res.trap.cause == 0x8000_0007
    assert res.trap.epc == emu.ram_base + 4  # instruction after wfi
    assert emu.instret == 1, "cycles spent in wfi do not retire"


@pytest.mark.parametrize("source_kind", ["path", "str", "bytes"])
def test_load_elf_runs_riscv_test(source_kind):
    path = ISA / "rv32ui-p-add.elf"
    source = {"path": path, "str": str(path), "bytes": path.read_bytes()}[source_kind]

    emu = Emulator(ram_size=64 * 1024)
    entry = emu.load_elf(source)
    assert entry == emu.pc == 0x8000_0000
    assert emu.symbol("tohost") == 0x8000_1000
    assert "write_tohost" in emu.symbols
    assert emu.symbol("does_not_exist") is None

    # The test environment ends in write_tohost with gp == 1 on success.
    emu.add_breakpoint(emu.symbol("write_tohost"))
    res = emu.run(100_000)
    assert res.reason == "breakpoint"
    assert emu.read_reg(3) == 1, f"riscv-test failed at case {emu.read_reg(3) >> 1}"


def test_load_elf_errors(tmp_path):
    emu = Emulator()
    with pytest.raises(ElfError, match="bad magic"):
        emu.load_elf(b"definitely not an ELF")
    with pytest.raises(FileNotFoundError):
        emu.load_elf(tmp_path / "missing.elf")
    small = Emulator(ram_size=1024)
    with pytest.raises(ElfError, match="cannot load"):
        small.load_elf(ISA / "rv32ui-p-add.elf")  # does not fit in 1 KiB
