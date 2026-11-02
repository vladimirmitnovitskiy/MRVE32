import pytest

from mrve32 import BusError, Coprocessor, CoprocessorCall, Emulator

MTVEC = 0x305
WFI = 0x1050_0073


def custom0(funct7, rs2, rs1, funct3, rd):
    return funct7 << 25 | rs2 << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | 0x0B


class Mac:
    """custom-0: funct3=0 rd += rs1*rs2; funct3=1 rd = mem[rs1]; funct3=2 jump to rs1."""

    def __init__(self):
        self.calls = 0

    def execute(self, call):
        self.calls += 1
        assert call.opcode == 0x0B
        a, b = call.read_reg(call.rs1), call.read_reg(call.rs2)
        if call.funct3 == 0:
            call.write_reg(call.rd, (call.read_reg(call.rd) + a * b) & 0xFFFF_FFFF)
        elif call.funct3 == 1:
            call.write_reg(call.rd, call.read_u32(a))
        elif call.funct3 == 2:
            call.next_pc = a
        else:
            return False


def setup(program):
    emu = Emulator()
    emu.write_csr(MTVEC, emu.ram_base + 0x100)
    emu.load_program(emu.ram_base + 0x100, [WFI])
    emu.load_program(emu.ram_base, program)
    return emu


def test_custom_instruction_reads_and_writes_registers():
    mac = Mac()
    assert isinstance(mac, Coprocessor)
    emu = setup(
        [
            0x0030_0093,  # addi x1, x0, 3
            0x0070_0113,  # addi x2, x0, 7
            custom0(0, 2, 1, 0, 3),  # x3 += x1*x2
            custom0(0, 2, 1, 0, 3),
            custom0(0, 2, 1, 0, 0),  # rd = x0: discarded
        ]
    )
    emu.set_coprocessor(mac)
    for _ in range(5):
        assert emu.step() is None
    assert (emu.read_reg(3), emu.read_reg(0), mac.calls) == (42, 0, 3)
    assert emu.pc == emu.ram_base + 20


def test_without_coprocessor_custom_is_illegal():
    word = custom0(0, 0, 0, 0, 1)
    emu = setup([word])
    trap = emu.step()
    assert (trap.cause, trap.tval) == (2, word)

    emu = setup([word])
    emu.set_coprocessor(Mac())
    emu.set_coprocessor(None)
    assert emu.step().cause == 2


def test_false_means_illegal_and_rolls_back():
    class Bad:
        def execute(self, call):
            call.write_reg(5, 123)
            return False

    word = custom0(0, 0, 0, 7, 5)
    emu = setup([word])
    emu.set_coprocessor(Bad())
    trap = emu.step()
    assert (trap.cause, trap.tval, trap.epc) == (2, word, emu.ram_base)
    assert emu.read_reg(5) == 0


def test_bus_access_and_access_fault():
    emu = setup([custom0(0, 0, 1, 1, 2)])  # x2 = mem[x1]
    emu.set_coprocessor(Mac())
    emu.write_u32(emu.ram_base + 0x200, 0xCAFE_F00D)
    emu.write_reg(1, emu.ram_base + 0x200)
    assert emu.step() is None
    assert emu.read_reg(2) == 0xCAFE_F00D

    emu = setup([custom0(0, 0, 1, 1, 2)])
    emu.set_coprocessor(Mac())
    emu.write_reg(1, 0x4000_0000)  # unmapped
    trap = emu.step()
    assert (trap.cause, trap.tval) == (5, 0x4000_0000)  # load access fault

    class Writer:
        def execute(self, call):
            call.write_u8(0x4000_0001, 1)

    emu = setup([custom0(0, 0, 0, 0, 0)])
    emu.set_coprocessor(Writer())
    assert (emu.step().cause, emu.read_csr(0x343)) == (7, 0x4000_0001)


def test_caught_bus_error_is_not_a_fault():
    class Probe:
        def execute(self, call):
            try:
                call.read_u32(0x4000_0000)
            except BusError:
                call.write_reg(call.rd, 1)

    emu = setup([custom0(0, 0, 0, 0, 4)])
    emu.set_coprocessor(Probe())
    assert emu.step() is None and emu.read_reg(4) == 1


def test_next_pc_branches_and_is_checked():
    emu = setup([custom0(0, 0, 1, 2, 0)])
    emu.set_coprocessor(Mac())
    emu.write_reg(1, emu.ram_base + 0x40)
    assert emu.step() is None and emu.pc == emu.ram_base + 0x40

    emu = setup([custom0(0, 0, 1, 2, 0)])
    emu.set_coprocessor(Mac())
    emu.write_reg(1, emu.ram_base + 0x41)
    trap = emu.step()
    assert (trap.cause, trap.tval) == (0, emu.ram_base + 0x41)  # misaligned


def test_python_exception_aborts_run():
    class Boom:
        def execute(self, call):
            raise KeyError("boom")

    emu = setup([custom0(0, 0, 0, 0, 0)])
    emu.set_coprocessor(Boom())
    with pytest.raises(KeyError, match="boom"):
        emu.run(100)


def test_call_object_is_invalid_after_execute():
    kept = []
    emu = setup([custom0(0, 0, 0, 0, 0)])
    base = emu.ram_base  # the Emulator itself is busy during execute()

    class Keeper:
        def execute(self, call):
            assert isinstance(call, CoprocessorCall)
            assert call.pc == base and call.next_pc == base + 4
            kept.append(call)

    emu.set_coprocessor(Keeper())
    emu.step()
    assert kept[0].word == custom0(0, 0, 0, 0, 0)  # plain fields stay readable
    with pytest.raises(RuntimeError, match="only valid inside execute"):
        kept[0].read_reg(1)
    with pytest.raises(RuntimeError):
        kept[0].write_u32(emu.ram_base, 0)
