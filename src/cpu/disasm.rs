//! Disassembler in GNU objdump style (ABI register names, common
//! pseudo-instructions, CSR names).

use std::fmt;

use super::csr;
use super::decode::decode;
use super::decode_c::decode_compressed;
use super::instructions::Instruction;
use crate::types::Reg;

const ABI_NAMES: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

/// ABI name of an integer register (`x10` → `"a0"`).
pub fn reg_name(reg: usize) -> &'static str {
    ABI_NAMES[reg & 31]
}

/// Name of a CSR implemented by this emulator, if any.
pub fn csr_name(csr: u16) -> Option<&'static str> {
    Some(match csr {
        csr::MSTATUS => "mstatus",
        csr::MISA => "misa",
        csr::MIE => "mie",
        csr::MTVEC => "mtvec",
        csr::MSTATUSH => "mstatush",
        csr::MCOUNTINHIBIT => "mcountinhibit",
        csr::MSCRATCH => "mscratch",
        csr::MEPC => "mepc",
        csr::MCAUSE => "mcause",
        csr::MTVAL => "mtval",
        csr::MIP => "mip",
        csr::MCYCLE => "mcycle",
        csr::MINSTRET => "minstret",
        csr::MCYCLEH => "mcycleh",
        csr::MINSTRETH => "minstreth",
        csr::CYCLE => "cycle",
        csr::INSTRET => "instret",
        csr::CYCLEH => "cycleh",
        csr::INSTRETH => "instreth",
        csr::MVENDORID => "mvendorid",
        csr::MARCHID => "marchid",
        csr::MIMPID => "mimpid",
        csr::MHARTID => "mhartid",
        csr::MCONFIGPTR => "mconfigptr",
        _ => return None,
    })
}

/// Whether an instruction word (or its low half) is a 16-bit instruction.
pub fn is_compressed(raw: u32) -> bool {
    raw & 0b11 != 0b11
}

/// Disassembles one instruction: a 32-bit word, or a 16-bit (C)
/// instruction in the low half when its low bits are not `0b11`. With `pc`,
/// branch and jump targets are shown as absolute addresses, otherwise as
/// offsets.
///
/// ```
/// use mrve32::disassemble;
/// assert_eq!(disassemble(0x00F00093, None), "li ra, 15");
/// assert_eq!(disassemble(0x0080006F, Some(0x8000_0000)), "j 0x80000008");
/// assert_eq!(disassemble(0x852e, None), "mv a0, a1"); // c.mv
/// assert_eq!(disassemble(0xFFFFFFFF, None), ".word 0xffffffff");
/// ```
pub fn disassemble(raw: u32, pc: Option<Reg>) -> String {
    if is_compressed(raw) {
        let half = raw as u16;
        return match decode_compressed(half) {
            Ok(inst) => Disasm {
                inst,
                pc,
                compressed: true,
            }
            .to_string(),
            Err(_) if half == 0 => "unimp".to_string(),
            Err(_) => format!(".short 0x{half:04x}"),
        };
    }
    match decode(raw) {
        Ok(inst) => Disasm {
            inst,
            pc,
            compressed: false,
        }
        .to_string(),
        Err(_) => format!(".word 0x{raw:08x}"),
    }
}

impl fmt::Display for Instruction {
    /// Formats the instruction; branch and jump targets as offsets.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Disasm {
            inst: *self,
            pc: None,
            compressed: false,
        }
        .fmt(f)
    }
}

struct Disasm {
    inst: Instruction,
    pc: Option<Reg>,
    /// Expanded from a 16-bit instruction (objdump prints `c.mv` as `mv`).
    compressed: bool,
}

impl Disasm {
    fn target(&self, imm: i32) -> String {
        match self.pc {
            Some(pc) => format!("0x{:x}", pc.wrapping_add(imm as u32)),
            None => format!("{imm}"),
        }
    }
}

fn csr_str(csr: u16) -> String {
    csr_name(csr).map_or_else(|| format!("0x{csr:03x}"), str::to_string)
}

impl fmt::Display for Disasm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Instruction as I;
        let r = reg_name;
        macro_rules! rtype {
            ($m:literal, $rd:expr, $rs1:expr, $rs2:expr) => {
                write!(f, "{} {}, {}, {}", $m, r($rd), r($rs1), r($rs2))
            };
        }
        macro_rules! itype {
            ($m:literal, $rd:expr, $rs1:expr, $imm:expr) => {
                write!(f, "{} {}, {}, {}", $m, r($rd), r($rs1), $imm)
            };
        }
        macro_rules! mem {
            ($m:literal, $reg:expr, $rs1:expr, $imm:expr) => {
                write!(f, "{} {}, {}({})", $m, r($reg), $imm, r($rs1))
            };
        }
        macro_rules! branch {
            ($m:literal, $rs1:expr, $rs2:expr, $imm:expr) => {
                write!(f, "{} {}, {}, {}", $m, r($rs1), r($rs2), self.target($imm))
            };
        }

        match self.inst {
            I::Add { rd, rs1: 0, rs2 } if self.compressed => {
                write!(f, "mv {}, {}", r(rd), r(rs2))
            }
            I::Add { rd, rs1, rs2 } => rtype!("add", rd, rs1, rs2),
            I::Sub { rd, rs1: 0, rs2 } => write!(f, "neg {}, {}", r(rd), r(rs2)),
            I::Sub { rd, rs1, rs2 } => rtype!("sub", rd, rs1, rs2),
            I::Sll { rd, rs1, rs2 } => rtype!("sll", rd, rs1, rs2),
            I::Slt { rd, rs1, rs2 } => rtype!("slt", rd, rs1, rs2),
            I::Sltu { rd, rs1: 0, rs2 } => write!(f, "snez {}, {}", r(rd), r(rs2)),
            I::Sltu { rd, rs1, rs2 } => rtype!("sltu", rd, rs1, rs2),
            I::Xor { rd, rs1, rs2 } => rtype!("xor", rd, rs1, rs2),
            I::Srl { rd, rs1, rs2 } => rtype!("srl", rd, rs1, rs2),
            I::Sra { rd, rs1, rs2 } => rtype!("sra", rd, rs1, rs2),
            I::Or { rd, rs1, rs2 } => rtype!("or", rd, rs1, rs2),
            I::And { rd, rs1, rs2 } => rtype!("and", rd, rs1, rs2),
            I::Mul { rd, rs1, rs2 } => rtype!("mul", rd, rs1, rs2),
            I::Mulh { rd, rs1, rs2 } => rtype!("mulh", rd, rs1, rs2),
            I::Mulhsu { rd, rs1, rs2 } => rtype!("mulhsu", rd, rs1, rs2),
            I::Mulhu { rd, rs1, rs2 } => rtype!("mulhu", rd, rs1, rs2),
            I::Div { rd, rs1, rs2 } => rtype!("div", rd, rs1, rs2),
            I::Divu { rd, rs1, rs2 } => rtype!("divu", rd, rs1, rs2),
            I::Rem { rd, rs1, rs2 } => rtype!("rem", rd, rs1, rs2),
            I::Remu { rd, rs1, rs2 } => rtype!("remu", rd, rs1, rs2),

            I::Addi {
                rd: 0,
                rs1: 0,
                imm: 0,
            } => write!(f, "nop"),
            I::Addi { rd, rs1: 0, imm } => write!(f, "li {}, {}", r(rd), imm),
            I::Addi { rd, rs1, imm: 0 } => write!(f, "mv {}, {}", r(rd), r(rs1)),
            I::Addi { rd, rs1, imm } => itype!("addi", rd, rs1, imm),
            I::Slti { rd, rs1, imm } => itype!("slti", rd, rs1, imm),
            I::Sltiu { rd, rs1, imm: 1 } => write!(f, "seqz {}, {}", r(rd), r(rs1)),
            I::Sltiu { rd, rs1, imm } => itype!("sltiu", rd, rs1, imm),
            I::Xori { rd, rs1, imm: -1 } => write!(f, "not {}, {}", r(rd), r(rs1)),
            I::Xori { rd, rs1, imm } => itype!("xori", rd, rs1, imm),
            I::Ori { rd, rs1, imm } => itype!("ori", rd, rs1, imm),
            I::Andi { rd, rs1, imm: 255 } => write!(f, "zext.b {}, {}", r(rd), r(rs1)),
            I::Andi { rd, rs1, imm } => itype!("andi", rd, rs1, imm),
            I::Slli { rd, rs1, imm } => itype!("slli", rd, rs1, format_args!("0x{:x}", imm & 0x1F)),
            I::Srli { rd, rs1, imm } => itype!("srli", rd, rs1, format_args!("0x{:x}", imm & 0x1F)),
            I::Srai { rd, rs1, imm } => itype!("srai", rd, rs1, format_args!("0x{:x}", imm & 0x1F)),

            I::Lb { rd, rs1, imm } => mem!("lb", rd, rs1, imm),
            I::Lh { rd, rs1, imm } => mem!("lh", rd, rs1, imm),
            I::Lw { rd, rs1, imm } => mem!("lw", rd, rs1, imm),
            I::Lbu { rd, rs1, imm } => mem!("lbu", rd, rs1, imm),
            I::Lhu { rd, rs1, imm } => mem!("lhu", rd, rs1, imm),
            I::Sb { rs1, rs2, imm } => mem!("sb", rs2, rs1, imm),
            I::Sh { rs1, rs2, imm } => mem!("sh", rs2, rs1, imm),
            I::Sw { rs1, rs2, imm } => mem!("sw", rs2, rs1, imm),

            I::Beq { rs1, rs2: 0, imm } => {
                write!(f, "beqz {}, {}", r(rs1), self.target(imm))
            }
            I::Beq { rs1, rs2, imm } => branch!("beq", rs1, rs2, imm),
            I::Bne { rs1, rs2: 0, imm } => {
                write!(f, "bnez {}, {}", r(rs1), self.target(imm))
            }
            I::Bne { rs1, rs2, imm } => branch!("bne", rs1, rs2, imm),
            I::Blt { rs1, rs2, imm } => branch!("blt", rs1, rs2, imm),
            I::Bge { rs1, rs2, imm } => branch!("bge", rs1, rs2, imm),
            I::Bltu { rs1, rs2, imm } => branch!("bltu", rs1, rs2, imm),
            I::Bgeu { rs1, rs2, imm } => branch!("bgeu", rs1, rs2, imm),

            I::Lui { rd, imm } => write!(f, "lui {}, 0x{:x}", r(rd), (imm as u32) >> 12),
            I::Auipc { rd, imm } => write!(f, "auipc {}, 0x{:x}", r(rd), (imm as u32) >> 12),

            I::Jal { rd: 0, imm } => write!(f, "j {}", self.target(imm)),
            I::Jal { rd: 1, imm } => write!(f, "jal {}", self.target(imm)),
            I::Jal { rd, imm } => write!(f, "jal {}, {}", r(rd), self.target(imm)),
            I::Jalr {
                rd: 0,
                rs1: 1,
                imm: 0,
            } => write!(f, "ret"),
            I::Jalr { rd: 0, rs1, imm: 0 } => write!(f, "jr {}", r(rs1)),
            I::Jalr { rd: 1, rs1, imm: 0 } => write!(f, "jalr {}", r(rs1)),
            I::Jalr { rd, rs1, imm } => mem!("jalr", rd, rs1, imm),

            I::Ecall => write!(f, "ecall"),
            I::Ebreak => write!(f, "ebreak"),
            I::Fence => write!(f, "fence"),
            I::FenceI => write!(f, "fence.i"),
            I::Mret => write!(f, "mret"),
            I::Wfi => write!(f, "wfi"),
            I::Custom { opcode, raw } => {
                let n = if opcode == 0x0B { 0 } else { 1 };
                write!(f, "custom-{n} 0x{raw:08x}")
            }

            I::Csrrs { rd, rs1: 0, csr } => match csr {
                csr::CYCLE => write!(f, "rdcycle {}", r(rd)),
                csr::CYCLEH => write!(f, "rdcycleh {}", r(rd)),
                csr::INSTRET => write!(f, "rdinstret {}", r(rd)),
                csr::INSTRETH => write!(f, "rdinstreth {}", r(rd)),
                _ => write!(f, "csrr {}, {}", r(rd), csr_str(csr)),
            },
            I::Csrrw { rd: 0, rs1, csr } => write!(f, "csrw {}, {}", csr_str(csr), r(rs1)),
            I::Csrrs { rd: 0, rs1, csr } => write!(f, "csrs {}, {}", csr_str(csr), r(rs1)),
            I::Csrrc { rd: 0, rs1, csr } => write!(f, "csrc {}, {}", csr_str(csr), r(rs1)),
            I::Csrrw { rd, rs1, csr } => {
                write!(f, "csrrw {}, {}, {}", r(rd), csr_str(csr), r(rs1))
            }
            I::Csrrs { rd, rs1, csr } => {
                write!(f, "csrrs {}, {}, {}", r(rd), csr_str(csr), r(rs1))
            }
            I::Csrrc { rd, rs1, csr } => {
                write!(f, "csrrc {}, {}, {}", r(rd), csr_str(csr), r(rs1))
            }
            I::Csrrwi { rd: 0, uimm, csr } => write!(f, "csrwi {}, {}", csr_str(csr), uimm),
            I::Csrrsi { rd: 0, uimm, csr } => write!(f, "csrsi {}, {}", csr_str(csr), uimm),
            I::Csrrci { rd: 0, uimm, csr } => write!(f, "csrci {}, {}", csr_str(csr), uimm),
            I::Csrrwi { rd, uimm, csr } => {
                write!(f, "csrrwi {}, {}, {}", r(rd), csr_str(csr), uimm)
            }
            I::Csrrsi { rd, uimm, csr } => {
                write!(f, "csrrsi {}, {}, {}", r(rd), csr_str(csr), uimm)
            }
            I::Csrrci { rd, uimm, csr } => {
                write!(f, "csrrci {}, {}, {}", r(rd), csr_str(csr), uimm)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected text is GNU objdump 2.46 output for each word at pc
    /// 0x80000000 (default options, i.e. with pseudo-instructions), modulo
    /// the space after commas.
    #[test]
    fn test_matches_objdump() {
        let pc = Some(0x8000_0000);
        let cases: &[(u32, &str)] = &[
            (0x00000013, "nop"),
            (0x00F00093, "li ra, 15"),
            (0x00050593, "mv a1, a0"),
            (0x01B10113, "addi sp, sp, 27"),
            (0x002081B3, "add gp, ra, sp"),
            (0x40B50533, "sub a0, a0, a1"),
            (0x40A00533, "neg a0, a0"),
            (0x02B50533, "mul a0, a0, a1"),
            (0x02B54533, "div a0, a0, a1"),
            (0xFFF54513, "not a0, a0"),
            (0x00153513, "seqz a0, a0"),
            (0x00A03533, "snez a0, a0"),
            (0x00351513, "slli a0, a0, 0x3"),
            (0x0ff7f693, "zext.b a3, a5"),
            (0x41F55513, "srai a0, a0, 0x1f"),
            (0x00812503, "lw a0, 8(sp)"),
            (0xFFC44583, "lbu a1, -4(s0)"),
            (0x00B12623, "sw a1, 12(sp)"),
            (0x00B50463, "beq a0, a1, 0x80000008"),
            (0x00050463, "beqz a0, 0x80000008"),
            (0xFE051EE3, "bnez a0, 0x7ffffffc"),
            (0x00B56463, "bltu a0, a1, 0x80000008"),
            (0x123452B7, "lui t0, 0x12345"),
            (0x00000297, "auipc t0, 0x0"),
            (0x0080006F, "j 0x80000008"),
            (0x008000EF, "jal 0x80000008"),
            (0x00008067, "ret"),
            (0x00028067, "jr t0"),
            (0x000280E7, "jalr t0"),
            (0x004282E7, "jalr t0, 4(t0)"),
            (0x00000073, "ecall"),
            (0x00100073, "ebreak"),
            (0x30200073, "mret"),
            (0x10500073, "wfi"),
            (0x0FF0000F, "fence"),
            (0x0000100F, "fence.i"),
            (0x30002573, "csrr a0, mstatus"),
            (0x30551073, "csrw mtvec, a0"),
            (0x30046073, "csrsi mstatus, 8"),
            (0x34151573, "csrrw a0, mepc, a0"),
            (0xC0002573, "rdcycle a0"),
            (0x7C002573, "csrr a0, 0x7c0"),
        ];
        for &(raw, text) in cases {
            assert_eq!(disassemble(raw, pc), text, "raw 0x{raw:08x}");
        }
    }

    #[test]
    fn test_offsets_without_pc_and_display() {
        assert_eq!(disassemble(0x00B50463, None), "beq a0, a1, 8");
        assert_eq!(disassemble(0xFE051EE3, None), "bnez a0, -4");
        let inst = decode(0x00812503).unwrap();
        assert_eq!(inst.to_string(), "lw a0, 8(sp)");
    }

    /// GNU objdump 2.46 on an ELF assembled with `-march=rv32imc` (binary
    /// mode would decode RV64C).
    #[test]
    fn test_compressed_matches_objdump() {
        let cases: &[(u32, Option<Reg>, &str)] = &[
            (0x2019, Some(0), "jal 0x6"),
            (0x3ffd, Some(2), "jal 0x0"),
            (0x852e, None, "mv a0, a1"),
            (0x0000, None, "unimp"),
            (0x2000, None, ".short 0x2000"),
            (0x7179, None, "addi sp, sp, -48"),
            (0x757d, None, "lui a0, 0xfffff"),
            (0x8105, None, "srli a0, a0, 0x1"),
            (0xdd7d, Some(0x16), "beqz a0, 0x14"),
            (0xe111, Some(0x18), "bnez a0, 0x1c"),
            (0xbffd, Some(0x1a), "j 0x18"),
            (0x1ffc, None, "addi a5, sp, 1020"),
            (0x50fe, None, "lw ra, 252(sp)"),
            (0xdffe, None, "sw t6, 252(sp)"),
            (0x0800, None, "addi s0, sp, 16"),
            (0x0001, None, "nop"),
            (0x557d, None, "li a0, -1"),
            (0x8082, None, "ret"),
            (0x9002, None, "ebreak"),
            (0x9502, None, "jalr a0"),
            (0x952e, None, "add a0, a0, a1"),
            (0xc62a, None, "sw a0, 12(sp)"),
        ];
        for &(raw, pc, text) in cases {
            assert_eq!(disassemble(raw, pc), text, "raw 0x{raw:04x}");
        }
        // The 32-bit add with rs1 = zero is not printed as mv.
        assert_eq!(disassemble(0x00b00533, None), "add a0, zero, a1");
    }

    #[test]
    fn test_invalid_and_custom() {
        assert_eq!(disassemble(0x0000_0000, None), "unimp");
        assert_eq!(disassemble(0x0000_007F, None), ".word 0x0000007f");
        assert_eq!(disassemble(0x0000_000B, None), "custom-0 0x0000000b");
    }
}
