//! RV32C (Zca) decoder: expands a 16-bit instruction into its 32-bit
//! equivalent [`Instruction`].
//!
//! Floating-point loads/stores (`c.flw`, `c.fld`, ...) are illegal (no F/D).
//! Reserved encodings are illegal; HINT encodings execute as the base
//! instruction, which has no architectural effect for them.

use super::instructions::Instruction;
use super::trap::Exception;

/// `x8..x15` register from a 3-bit field.
fn creg(field: u16) -> usize {
    8 + (field & 0b111) as usize
}

fn bit(raw: u16, n: u32) -> u32 {
    ((raw >> n) & 1) as u32
}

fn bits(raw: u16, hi: u32, lo: u32) -> u32 {
    ((raw >> lo) as u32) & ((1 << (hi - lo + 1)) - 1)
}

/// Sign-extends the low `width` bits of `v`.
fn sext(v: u32, width: u32) -> i32 {
    let shift = 32 - width;
    ((v << shift) as i32) >> shift
}

/// 6-bit immediate of CI-format instructions: imm[5] = bit 12, imm[4:0] = bits 6:2.
fn ci_imm(raw: u16) -> i32 {
    sext(bit(raw, 12) << 5 | bits(raw, 6, 2), 6)
}

/// Offset of c.j / c.jal: [11|4|9:8|10|6|7|3:1|5] = bits [12|11|10:9|8|7|6|5:3|2].
fn cj_offset(raw: u16) -> i32 {
    let v = bit(raw, 12) << 11
        | bit(raw, 11) << 4
        | bits(raw, 10, 9) << 8
        | bit(raw, 8) << 10
        | bit(raw, 7) << 6
        | bit(raw, 6) << 7
        | bits(raw, 5, 3) << 1
        | bit(raw, 2) << 5;
    sext(v, 12)
}

/// Offset of c.beqz / c.bnez: [8|4:3] = bits [12|11:10], [7:6|2:1|5] = bits [6:5|4:3|2].
fn cb_offset(raw: u16) -> i32 {
    let v = bit(raw, 12) << 8
        | bits(raw, 11, 10) << 3
        | bits(raw, 6, 5) << 6
        | bits(raw, 4, 3) << 1
        | bit(raw, 2) << 5;
    sext(v, 9)
}

/// Offset of c.lw / c.sw: [5:3] = bits 12:10, [2] = bit 6, [6] = bit 5.
fn clw_offset(raw: u16) -> i32 {
    (bits(raw, 12, 10) << 3 | bit(raw, 6) << 2 | bit(raw, 5) << 6) as i32
}

pub fn decode_compressed(raw: u16) -> Result<Instruction, Exception> {
    use Instruction as I;
    let illegal = || Exception::IllegalInstruction(raw.into());
    let funct3 = raw >> 13;
    let rd = ((raw >> 7) & 0x1F) as usize;
    let rs2 = ((raw >> 2) & 0x1F) as usize;
    let rd_p = creg(raw >> 2); // rd' / rs2' in bits 4:2
    let rs1_p = creg(raw >> 7); // rs1' / rd' in bits 9:7

    Ok(match (raw & 0b11, funct3) {
        // --- Quadrant 0 ---
        (0b00, 0b000) => {
            // c.addi4spn: nzuimm[5:4|9:6|2|3] = bits [12:11|10:7|6|5]
            let imm = bits(raw, 12, 11) << 4
                | bits(raw, 10, 7) << 6
                | bit(raw, 6) << 2
                | bit(raw, 5) << 3;
            if imm == 0 {
                return Err(illegal()); // includes the all-zero instruction
            }
            I::Addi {
                rd: rd_p,
                rs1: 2,
                imm: imm as i32,
            }
        }
        (0b00, 0b010) => I::Lw {
            rd: rd_p,
            rs1: rs1_p,
            imm: clw_offset(raw),
        },
        (0b00, 0b110) => I::Sw {
            rs1: rs1_p,
            rs2: rd_p,
            imm: clw_offset(raw),
        },
        // c.fld, c.flw, reserved, c.fsd, c.fsw
        (0b00, _) => return Err(illegal()),

        // --- Quadrant 1 ---
        (0b01, 0b000) => I::Addi {
            rd,
            rs1: rd,
            imm: ci_imm(raw),
        },
        (0b01, 0b001) => I::Jal {
            rd: 1,
            imm: cj_offset(raw),
        },
        (0b01, 0b010) => I::Addi {
            rd,
            rs1: 0,
            imm: ci_imm(raw),
        },
        (0b01, 0b011) if rd == 2 => {
            // c.addi16sp: nzimm[9] = bit 12, [4|6|8:7|5] = bits [6|5|4:3|2]
            let v = bit(raw, 12) << 9
                | bit(raw, 6) << 4
                | bit(raw, 5) << 6
                | bits(raw, 4, 3) << 7
                | bit(raw, 2) << 5;
            if v == 0 {
                return Err(illegal());
            }
            I::Addi {
                rd: 2,
                rs1: 2,
                imm: sext(v, 10),
            }
        }
        (0b01, 0b011) => {
            // c.lui: nzimm[17] = bit 12, nzimm[16:12] = bits 6:2
            let imm = ci_imm(raw);
            if imm == 0 {
                return Err(illegal());
            }
            I::Lui { rd, imm: imm << 12 }
        }
        (0b01, 0b100) => {
            let shamt = bits(raw, 6, 2) as i32;
            match bits(raw, 11, 10) {
                // shamt[5] = 1 is reserved on RV32.
                0b00 | 0b01 if bit(raw, 12) != 0 => return Err(illegal()),
                0b00 => I::Srli {
                    rd: rs1_p,
                    rs1: rs1_p,
                    imm: shamt,
                },
                0b01 => I::Srai {
                    rd: rs1_p,
                    rs1: rs1_p,
                    imm: shamt | 0x400,
                },
                0b10 => I::Andi {
                    rd: rs1_p,
                    rs1: rs1_p,
                    imm: ci_imm(raw),
                },
                // c.subw / c.addw (RV64) and the Zcb space: not supported.
                _ if bit(raw, 12) != 0 => return Err(illegal()),
                _ => {
                    let (rd, rs1, rs2) = (rs1_p, rs1_p, rd_p);
                    match bits(raw, 6, 5) {
                        0b00 => I::Sub { rd, rs1, rs2 },
                        0b01 => I::Xor { rd, rs1, rs2 },
                        0b10 => I::Or { rd, rs1, rs2 },
                        _ => I::And { rd, rs1, rs2 },
                    }
                }
            }
        }
        (0b01, 0b101) => I::Jal {
            rd: 0,
            imm: cj_offset(raw),
        },
        (0b01, 0b110) => I::Beq {
            rs1: rs1_p,
            rs2: 0,
            imm: cb_offset(raw),
        },
        (0b01, _) => I::Bne {
            rs1: rs1_p,
            rs2: 0,
            imm: cb_offset(raw),
        },

        // --- Quadrant 2 ---
        (0b10, 0b000) => {
            if bit(raw, 12) != 0 {
                return Err(illegal()); // shamt[5] = 1 is reserved on RV32
            }
            I::Slli {
                rd,
                rs1: rd,
                imm: bits(raw, 6, 2) as i32,
            }
        }
        (0b10, 0b010) => {
            if rd == 0 {
                return Err(illegal());
            }
            // c.lwsp: uimm[5] = bit 12, [4:2] = bits 6:4, [7:6] = bits 3:2
            let imm = bit(raw, 12) << 5 | bits(raw, 6, 4) << 2 | bits(raw, 3, 2) << 6;
            I::Lw {
                rd,
                rs1: 2,
                imm: imm as i32,
            }
        }
        (0b10, 0b100) => match (bit(raw, 12), rd, rs2) {
            (0, 0, 0) => return Err(illegal()), // c.jr x0 is reserved
            (0, rs1, 0) => I::Jalr { rd: 0, rs1, imm: 0 },
            (0, rd, rs2) => I::Add { rd, rs1: 0, rs2 },
            (_, 0, 0) => I::Ebreak,
            (_, rs1, 0) => I::Jalr { rd: 1, rs1, imm: 0 },
            (_, rd, rs2) => I::Add { rd, rs1: rd, rs2 },
        },
        (0b10, 0b110) => {
            // c.swsp: uimm[5:2] = bits 12:9, [7:6] = bits 8:7
            let imm = bits(raw, 12, 9) << 2 | bits(raw, 8, 7) << 6;
            I::Sw {
                rs1: 2,
                rs2,
                imm: imm as i32,
            }
        }
        // c.fldsp, c.flwsp, c.fsdsp, c.fswsp
        (0b10, _) => return Err(illegal()),

        // Quadrant 3 is a 32-bit instruction.
        _ => return Err(illegal()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use Instruction as I;

    fn ok(raw: u16) -> Instruction {
        decode_compressed(raw).unwrap_or_else(|e| panic!("0x{raw:04x}: {e}"))
    }

    fn illegal(raw: u16) {
        assert_eq!(
            decode_compressed(raw),
            Err(Exception::IllegalInstruction(raw.into())),
            "0x{raw:04x}"
        );
    }

    // Encodings from GNU as (`-march=rv32imc`), see the cross-check with
    // objdump in disasm.rs.
    #[test]
    fn test_quadrant0() {
        // c.addi4spn s0, sp, 16
        assert_eq!(
            ok(0x0800),
            I::Addi {
                rd: 8,
                rs1: 2,
                imm: 16
            }
        );
        // c.addi4spn a5, sp, 1020
        assert_eq!(
            ok(0x1ffc),
            I::Addi {
                rd: 15,
                rs1: 2,
                imm: 1020
            }
        );
        // c.lw a0, 4(a1)
        assert_eq!(
            ok(0x41c8),
            I::Lw {
                rd: 10,
                rs1: 11,
                imm: 4
            }
        );
        // c.lw a5, 124(a5)
        assert_eq!(
            ok(0x5ffc),
            I::Lw {
                rd: 15,
                rs1: 15,
                imm: 124
            }
        );
        // c.sw a0, 64(a1)
        assert_eq!(
            ok(0xc1a8),
            I::Sw {
                rs1: 11,
                rs2: 10,
                imm: 64
            }
        );
        illegal(0x0000);
        illegal(0x2000); // c.fld
        illegal(0x6000); // c.flw
        illegal(0x8000); // reserved
        illegal(0xe000); // c.fsw
    }

    #[test]
    fn test_quadrant1() {
        assert_eq!(
            ok(0x0001),
            I::Addi {
                rd: 0,
                rs1: 0,
                imm: 0
            }
        ); // c.nop
        assert_eq!(
            ok(0x157d),
            I::Addi {
                rd: 10,
                rs1: 10,
                imm: -1
            }
        );
        assert_eq!(ok(0x2001), I::Jal { rd: 1, imm: 0 });
        assert_eq!(ok(0x3ffd), I::Jal { rd: 1, imm: -2 });
        assert_eq!(ok(0x2fe5), I::Jal { rd: 1, imm: 2040 });
        assert_eq!(
            ok(0x557d),
            I::Addi {
                rd: 10,
                rs1: 0,
                imm: -1
            }
        ); // c.li
        assert_eq!(
            ok(0x4515),
            I::Addi {
                rd: 10,
                rs1: 0,
                imm: 5
            }
        );
        assert_eq!(
            ok(0x7179),
            I::Addi {
                rd: 2,
                rs1: 2,
                imm: -48
            }
        ); // c.addi16sp
        assert_eq!(
            ok(0x6105),
            I::Addi {
                rd: 2,
                rs1: 2,
                imm: 32
            }
        );
        assert_eq!(
            ok(0x6505),
            I::Lui {
                rd: 10,
                imm: 1 << 12
            }
        ); // c.lui
        assert_eq!(
            ok(0x757d),
            I::Lui {
                rd: 10,
                imm: -1 << 12
            }
        );
        assert_eq!(
            ok(0x8105),
            I::Srli {
                rd: 10,
                rs1: 10,
                imm: 1
            }
        );
        assert_eq!(
            ok(0x857d),
            I::Srai {
                rd: 10,
                rs1: 10,
                imm: 0x41F
            }
        );
        assert_eq!(
            ok(0x893d),
            I::Andi {
                rd: 10,
                rs1: 10,
                imm: 15
            }
        );
        assert_eq!(
            ok(0x8d0d),
            I::Sub {
                rd: 10,
                rs1: 10,
                rs2: 11
            }
        );
        assert_eq!(
            ok(0x8d2d),
            I::Xor {
                rd: 10,
                rs1: 10,
                rs2: 11
            }
        );
        assert_eq!(
            ok(0x8d4d),
            I::Or {
                rd: 10,
                rs1: 10,
                rs2: 11
            }
        );
        assert_eq!(
            ok(0x8d6d),
            I::And {
                rd: 10,
                rs1: 10,
                rs2: 11
            }
        );
        assert_eq!(ok(0xa001), I::Jal { rd: 0, imm: 0 }); // c.j
        assert_eq!(ok(0xbffd), I::Jal { rd: 0, imm: -2 });
        assert_eq!(
            ok(0xc111),
            I::Beq {
                rs1: 10,
                rs2: 0,
                imm: 4
            }
        );
        assert_eq!(
            ok(0xdd7d),
            I::Beq {
                rs1: 10,
                rs2: 0,
                imm: -2
            }
        );
        assert_eq!(
            ok(0xe111),
            I::Bne {
                rs1: 10,
                rs2: 0,
                imm: 4
            }
        );
        assert_eq!(
            ok(0xfd7d),
            I::Bne {
                rs1: 10,
                rs2: 0,
                imm: -2
            }
        );
        illegal(0x6101); // c.addi16sp with nzimm = 0
        illegal(0x6501); // c.lui with nzimm = 0
        illegal(0x9105); // c.srli with shamt[5] = 1
        illegal(0x9d0d); // c.subw (RV64)
    }

    #[test]
    fn test_quadrant2() {
        assert_eq!(
            ok(0x050e),
            I::Slli {
                rd: 10,
                rs1: 10,
                imm: 3
            }
        );
        assert_eq!(
            ok(0x4532),
            I::Lw {
                rd: 10,
                rs1: 2,
                imm: 12
            }
        ); // c.lwsp
        assert_eq!(
            ok(0x50fe),
            I::Lw {
                rd: 1,
                rs1: 2,
                imm: 252
            }
        );
        assert_eq!(
            ok(0x8082),
            I::Jalr {
                rd: 0,
                rs1: 1,
                imm: 0
            }
        ); // ret
        assert_eq!(
            ok(0x852e),
            I::Add {
                rd: 10,
                rs1: 0,
                rs2: 11
            }
        ); // c.mv
        assert_eq!(ok(0x9002), I::Ebreak);
        assert_eq!(
            ok(0x9502),
            I::Jalr {
                rd: 1,
                rs1: 10,
                imm: 0
            }
        );
        assert_eq!(
            ok(0x952e),
            I::Add {
                rd: 10,
                rs1: 10,
                rs2: 11
            }
        );
        assert_eq!(
            ok(0xc62a),
            I::Sw {
                rs1: 2,
                rs2: 10,
                imm: 12
            }
        ); // c.swsp
        assert_eq!(
            ok(0xdffe),
            I::Sw {
                rs1: 2,
                rs2: 31,
                imm: 252
            }
        );
        illegal(0x1506); // c.slli with shamt[5] = 1
        illegal(0x4002); // c.lwsp x0
        illegal(0x8002); // c.jr x0
        illegal(0x2002); // c.fldsp
        illegal(0x6002); // c.flwsp
        illegal(0xe002); // c.fswsp
    }
}
