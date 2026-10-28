use crate::cpu::{CpuError, instructions::Instruction};

pub fn decode(raw: u32) -> Result<Instruction, CpuError> {
    let opcode = (raw & 0x7F) as u8;
    let rd = ((raw >> 7) & 0x1F) as usize;
    let funct3 = ((raw >> 12) & 0x07) as u8;
    let rs1 = ((raw >> 15) & 0x1F) as usize;
    let rs2 = ((raw >> 20) & 0x1F) as usize;
    let funct7 = ((raw >> 25) & 0x7F) as u8;
    let imm = (raw as i32) >> 20;
    let imm_s = ((raw as i32) >> 25 << 5) | (((raw >> 7) & 0x1F) as i32);
    let bit12 = (raw as i32) >> 31 << 12;
    let bit11 = (((raw >> 7) & 0x01) as i32) << 11;
    let bits10_5 = (((raw >> 25) & 0x3F) as i32) << 5;
    let bits4_1 = (((raw >> 8) & 0x0F) as i32) << 1;
    let imm_b = bit12 | bit11 | bits10_5 | bits4_1;
    let bit20 = (raw as i32) >> 31 << 20;
    let bits19_12 = (((raw >> 12) & 0xFF) as i32) << 12;
    let bit11 = (((raw >> 20) & 0x01) as i32) << 11;
    let bits10_1 = (((raw >> 21) & 0x3FF) as i32) << 1;
    let imm_j = bit20 | bits19_12 | bit11 | bits10_1;
    let imm_u = (raw & 0xFFFF_F000) as i32;

    match opcode {
        0x33 => match (funct3, funct7) {
            (0x0, 0x00) => Ok(Instruction::Add { rd, rs1, rs2 }),
            (0x0, 0x20) => Ok(Instruction::Sub { rd, rs1, rs2 }),
            (0x1, 0x00) => Ok(Instruction::Sll { rd, rs1, rs2 }),
            (0x2, 0x00) => Ok(Instruction::Slt { rd, rs1, rs2 }),
            (0x3, 0x00) => Ok(Instruction::Sltu { rd, rs1, rs2 }),
            (0x4, 0x00) => Ok(Instruction::Xor { rd, rs1, rs2 }),
            (0x5, 0x00) => Ok(Instruction::Srl { rd, rs1, rs2 }),
            (0x5, 0x20) => Ok(Instruction::Sra { rd, rs1, rs2 }),
            (0x6, 0x00) => Ok(Instruction::Or { rd, rs1, rs2 }),
            (0x7, 0x00) => Ok(Instruction::And { rd, rs1, rs2 }),
            _ => Err(CpuError::IllegalInstruction(raw)),
        },
        0x13 => match funct3 {
            0x0 => Ok(Instruction::Addi { rd, rs1, imm }),
            0x2 => Ok(Instruction::Slti { rd, rs1, imm }),
            0x3 => Ok(Instruction::Sltiu { rd, rs1, imm }),
            0x4 => Ok(Instruction::Xori { rd, rs1, imm }),
            0x6 => Ok(Instruction::Ori { rd, rs1, imm }),
            0x7 => Ok(Instruction::Andi { rd, rs1, imm }),

            0x1 => {
                if funct7 == 0x00 {
                    Ok(Instruction::Slli { rd, rs1, imm })
                } else {
                    Err(CpuError::IllegalInstruction(raw))
                }
            }

            0x5 => match funct7 {
                0x00 => Ok(Instruction::Srli { rd, rs1, imm }),
                0x20 => Ok(Instruction::Srai { rd, rs1, imm }),
                _ => Err(CpuError::IllegalInstruction(raw)),
            },

            _ => Err(CpuError::IllegalInstruction(raw)),
        },
        0x03 => match funct3 {
            0x0 => Ok(Instruction::Lb { rd, rs1, imm }),
            0x1 => Ok(Instruction::Lh { rd, rs1, imm }),
            0x2 => Ok(Instruction::Lw { rd, rs1, imm }),
            0x4 => Ok(Instruction::Lbu { rd, rs1, imm }),
            0x5 => Ok(Instruction::Lhu { rd, rs1, imm }),
            _ => Err(CpuError::IllegalInstruction(raw)),
        },
        0x67 => match funct3 {
            0x0 => Ok(Instruction::Jalr { rd, rs1, imm }),
            _ => Err(CpuError::IllegalInstruction(raw)),
        },
        0x0B | 0x2B => Ok(Instruction::Custom { opcode, raw }),
        0x23 => match funct3 {
            0x0 => Ok(Instruction::Sb {
                rs1,
                rs2,
                imm: imm_s,
            }),
            0x1 => Ok(Instruction::Sh {
                rs1,
                rs2,
                imm: imm_s,
            }),
            0x2 => Ok(Instruction::Sw {
                rs1,
                rs2,
                imm: imm_s,
            }),
            _ => Err(CpuError::IllegalInstruction(raw)),
        },
        0x63 => match funct3 {
            0x0 => Ok(Instruction::Beq {
                rs1,
                rs2,
                imm: imm_b,
            }),
            0x1 => Ok(Instruction::Bne {
                rs1,
                rs2,
                imm: imm_b,
            }),
            0x4 => Ok(Instruction::Blt {
                rs1,
                rs2,
                imm: imm_b,
            }),
            0x5 => Ok(Instruction::Bge {
                rs1,
                rs2,
                imm: imm_b,
            }),
            0x6 => Ok(Instruction::Bltu {
                rs1,
                rs2,
                imm: imm_b,
            }),
            0x7 => Ok(Instruction::Bgeu {
                rs1,
                rs2,
                imm: imm_b,
            }),
            _ => Err(CpuError::IllegalInstruction(raw)),
        },
        0x6F => Ok(Instruction::Jal { rd, imm: imm_j }),
        0x37 => Ok(Instruction::Lui { rd, imm: imm_u }),
        0x17 => Ok(Instruction::Auipc { rd, imm: imm_u }),
        0x73 =>  {
            let csr = ((raw >> 20) & 0xFFF) as u16;
            let uimm = (raw >> 15) & 0x1F;

            match funct3 {
                0x0 => match raw {
                    0x0000_0073 => Ok(Instruction::Ecall),
                    0x0010_0073 => Ok(Instruction::Ebreak),
                    0x3020_0073 => Ok(Instruction::Mret),
                    _ => Err(CpuError::IllegalInstruction(raw)),
                },
                0x1 => Ok(Instruction::Csrrw { rd, rs1, csr }),
                0x2 => Ok(Instruction::Csrrs { rd, rs1, csr }),
                0x3 => Ok(Instruction::Csrrc { rd, rs1, csr }),
                0x5 => Ok(Instruction::Csrrwi { rd, uimm, csr }),
                0x6 => Ok(Instruction::Csrrsi { rd, uimm, csr }),
                0x7 => Ok(Instruction::Csrrci { rd, uimm, csr }),
                _ => Err(CpuError::IllegalInstruction(raw)),
            }
        },
        0x0F => Ok(Instruction::Fence),
        _ => Err(CpuError::IllegalInstruction(raw)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Вспомогательные функции кодирования инструкций ---

    fn encode_r_type(opcode: u8, rd: usize, funct3: u8, rs1: usize, rs2: usize, funct7: u8) -> u32 {
        ((funct7 as u32 & 0x7F) << 25)
            | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x07) << 12)
            | ((rd as u32 & 0x1F) << 7)
            | (opcode as u32 & 0x7F)
    }

    fn encode_i_type(opcode: u8, rd: usize, funct3: u8, rs1: usize, imm: i32) -> u32 {
        (((imm as u32) & 0xFFF) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x07) << 12)
            | ((rd as u32 & 0x1F) << 7)
            | (opcode as u32 & 0x7F)
    }

    fn encode_s_type(opcode: u8, funct3: u8, rs1: usize, rs2: usize, imm: i32) -> u32 {
        let imm_u = imm as u32;
        let imm_11_5 = (imm_u >> 5) & 0x7F;
        let imm_4_0 = imm_u & 0x1F;
        (imm_11_5 << 25)
            | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x07) << 12)
            | (imm_4_0 << 7)
            | (opcode as u32 & 0x7F)
    }

    fn encode_b_type(opcode: u8, funct3: u8, rs1: usize, rs2: usize, imm: i32) -> u32 {
        let imm_u = imm as u32;
        let bit_12 = (imm_u >> 12) & 0x1;
        let bit_11 = (imm_u >> 11) & 0x1;
        let bit_10_5 = (imm_u >> 5) & 0x3F;
        let bit_4_1 = (imm_u >> 1) & 0xF;

        (bit_12 << 31)
            | (bit_10_5 << 25)
            | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x07) << 12)
            | (bit_4_1 << 8)
            | (bit_11 << 7)
            | (opcode as u32 & 0x7F)
    }

    fn encode_u_type(opcode: u8, rd: usize, imm: i32) -> u32 {
        ((imm as u32) & 0xFFFF_F000) | ((rd as u32 & 0x1F) << 7) | (opcode as u32 & 0x7F)
    }

    fn encode_j_type(opcode: u8, rd: usize, imm: i32) -> u32 {
        let imm_u = imm as u32;
        let bit_20 = (imm_u >> 20) & 0x1;
        let bit_19_12 = (imm_u >> 12) & 0xFF;
        let bit_11 = (imm_u >> 11) & 0x1;
        let bit_10_1 = (imm_u >> 1) & 0x3FF;

        (bit_20 << 31)
            | (bit_10_1 << 21)
            | (bit_11 << 20)
            | (bit_19_12 << 12)
            | ((rd as u32 & 0x1F) << 7)
            | (opcode as u32 & 0x7F)
    }

    const OPCODE_R_TYPE: u8 = 0x33;
    const OPCODE_I_ALU: u8 = 0x13;
    const OPCODE_LOAD: u8 = 0x03;
    const OPCODE_JALR: u8 = 0x67;
    const OPCODE_STORE: u8 = 0x23;
    const OPCODE_BRANCH: u8 = 0x63;
    const OPCODE_LUI: u8 = 0x37;
    const OPCODE_AUIPC: u8 = 0x17;
    const OPCODE_JAL: u8 = 0x6F;

    // --- Существующие тесты R-типа ---

    #[test]
    fn test_decode_all_valid_r_type_instructions() {
        let cases = [
            (
                0x0,
                0x00,
                Instruction::Add {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x0,
                0x20,
                Instruction::Sub {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x1,
                0x00,
                Instruction::Sll {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x2,
                0x00,
                Instruction::Slt {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x3,
                0x00,
                Instruction::Sltu {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x4,
                0x00,
                Instruction::Xor {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x5,
                0x00,
                Instruction::Srl {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x5,
                0x20,
                Instruction::Sra {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x6,
                0x00,
                Instruction::Or {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
            (
                0x7,
                0x00,
                Instruction::And {
                    rd: 1,
                    rs1: 2,
                    rs2: 3,
                },
            ),
        ];

        for (funct3, funct7, expected) in cases {
            let raw = encode_r_type(OPCODE_R_TYPE, 1, funct3, 2, 3, funct7);
            assert_eq!(decode(raw), Ok(expected));
        }
    }

    #[test]
    fn test_register_bitfields_extraction() {
        let raw = encode_r_type(OPCODE_R_TYPE, 31, 0x0, 15, 0, 0x00);
        assert_eq!(
            decode(raw),
            Ok(Instruction::Add {
                rd: 31,
                rs1: 15,
                rs2: 0,
            })
        );
    }

    #[test]
    fn test_known_machine_code() {
        // add x10, x11, x12
        assert_eq!(
            decode(0x00C5_8533),
            Ok(Instruction::Add {
                rd: 10,
                rs1: 11,
                rs2: 12,
            })
        );

        // sub x1, x2, x3
        assert_eq!(
            decode(0x4031_00B3),
            Ok(Instruction::Sub {
                rd: 1,
                rs1: 2,
                rs2: 3,
            })
        );
    }

    #[test]
    fn test_illegal_opcode() {
        let raw_invalid = 0x0000_007F;
        assert_eq!(
            decode(raw_invalid),
            Err(CpuError::IllegalInstruction(raw_invalid))
        );

        let raw_zero = 0x0000_0000;
        assert_eq!(
            decode(raw_zero),
            Err(CpuError::IllegalInstruction(raw_zero))
        );
    }

    #[test]
    fn test_illegal_funct3_funct7_combinations() {
        let invalid_funct7 = encode_r_type(OPCODE_R_TYPE, 1, 0x0, 2, 3, 0x01);
        assert_eq!(
            decode(invalid_funct7),
            Err(CpuError::IllegalInstruction(invalid_funct7))
        );

        let invalid_sll = encode_r_type(OPCODE_R_TYPE, 1, 0x1, 2, 3, 0x20);
        assert_eq!(
            decode(invalid_sll),
            Err(CpuError::IllegalInstruction(invalid_sll))
        );
    }

    // --- Новые тесты для I-type ALU (0x13) ---

    #[test]
    fn test_decode_i_type_alu() {
        let cases = [
            (
                0x0,
                Instruction::Addi {
                    rd: 5,
                    rs1: 6,
                    imm: 42,
                },
            ),
            (
                0x2,
                Instruction::Slti {
                    rd: 5,
                    rs1: 6,
                    imm: 42,
                },
            ),
            (
                0x3,
                Instruction::Sltiu {
                    rd: 5,
                    rs1: 6,
                    imm: 42,
                },
            ),
            (
                0x4,
                Instruction::Xori {
                    rd: 5,
                    rs1: 6,
                    imm: 42,
                },
            ),
            (
                0x6,
                Instruction::Ori {
                    rd: 5,
                    rs1: 6,
                    imm: 42,
                },
            ),
            (
                0x7,
                Instruction::Andi {
                    rd: 5,
                    rs1: 6,
                    imm: 42,
                },
            ),
        ];

        for (funct3, expected) in cases {
            let raw = encode_i_type(OPCODE_I_ALU, 5, funct3, 6, 42);
            assert_eq!(decode(raw), Ok(expected));
        }

        // Проверка отрицательного immediate (знаковое расширение)
        let raw_neg = encode_i_type(OPCODE_I_ALU, 1, 0x0, 2, -1);
        assert_eq!(
            decode(raw_neg),
            Ok(Instruction::Addi {
                rd: 1,
                rs1: 2,
                imm: -1
            })
        );
    }

    #[test]
    fn test_decode_i_type_shifts() {
        // slli: funct7 = 0x00, shamt = 4
        let raw_slli = encode_r_type(OPCODE_I_ALU, 2, 0x1, 3, 4, 0x00);
        assert_eq!(
            decode(raw_slli),
            Ok(Instruction::Slli {
                rd: 2,
                rs1: 3,
                imm: 4
            })
        );

        // srli: funct7 = 0x00, shamt = 5
        let raw_srli = encode_r_type(OPCODE_I_ALU, 2, 0x5, 3, 5, 0x00);
        assert_eq!(
            decode(raw_srli),
            Ok(Instruction::Srli {
                rd: 2,
                rs1: 3,
                imm: 5
            })
        );

        // srai: funct7 = 0x20, shamt = 6
        let raw_srai = encode_r_type(OPCODE_I_ALU, 2, 0x5, 3, 6, 0x20);
        assert_eq!(
            decode(raw_srai),
            Ok(Instruction::Srai {
                rd: 2,
                rs1: 3,
                imm: 0x406
            })
        );

        // Невалидный funct7 для slli
        let invalid_slli = encode_r_type(OPCODE_I_ALU, 2, 0x1, 3, 4, 0x20);
        assert_eq!(
            decode(invalid_slli),
            Err(CpuError::IllegalInstruction(invalid_slli))
        );

        // Невалидный funct7 для srli/srai
        let invalid_shift = encode_r_type(OPCODE_I_ALU, 2, 0x5, 3, 4, 0x01);
        assert_eq!(
            decode(invalid_shift),
            Err(CpuError::IllegalInstruction(invalid_shift))
        );
    }

    // --- Тесты для Load инструкций (0x03) и JALR (0x67) ---

    #[test]
    fn test_decode_load_instructions() {
        let cases = [
            (
                0x0,
                Instruction::Lb {
                    rd: 4,
                    rs1: 5,
                    imm: -16,
                },
            ),
            (
                0x1,
                Instruction::Lh {
                    rd: 4,
                    rs1: 5,
                    imm: -16,
                },
            ),
            (
                0x2,
                Instruction::Lw {
                    rd: 4,
                    rs1: 5,
                    imm: -16,
                },
            ),
            (
                0x4,
                Instruction::Lbu {
                    rd: 4,
                    rs1: 5,
                    imm: -16,
                },
            ),
            (
                0x5,
                Instruction::Lhu {
                    rd: 4,
                    rs1: 5,
                    imm: -16,
                },
            ),
        ];

        for (funct3, expected) in cases {
            let raw = encode_i_type(OPCODE_LOAD, 4, funct3, 5, -16);
            assert_eq!(decode(raw), Ok(expected));
        }

        // Невалидный funct3 (0x3 или 0x7)
        let invalid_load = encode_i_type(OPCODE_LOAD, 4, 0x3, 5, 0);
        assert_eq!(
            decode(invalid_load),
            Err(CpuError::IllegalInstruction(invalid_load))
        );
    }

    #[test]
    fn test_decode_jalr() {
        let raw = encode_i_type(OPCODE_JALR, 1, 0x0, 2, 100);
        assert_eq!(
            decode(raw),
            Ok(Instruction::Jalr {
                rd: 1,
                rs1: 2,
                imm: 100
            })
        );

        let invalid_jalr = encode_i_type(OPCODE_JALR, 1, 0x1, 2, 100);
        assert_eq!(
            decode(invalid_jalr),
            Err(CpuError::IllegalInstruction(invalid_jalr))
        );
    }

    // --- Тесты для Store инструкций (0x23) ---

    #[test]
    fn test_decode_store_instructions() {
        let cases = [
            (
                0x0,
                Instruction::Sb {
                    rs1: 8,
                    rs2: 9,
                    imm: -40,
                },
            ),
            (
                0x1,
                Instruction::Sh {
                    rs1: 8,
                    rs2: 9,
                    imm: -40,
                },
            ),
            (
                0x2,
                Instruction::Sw {
                    rs1: 8,
                    rs2: 9,
                    imm: -40,
                },
            ),
        ];

        for (funct3, expected) in cases {
            let raw = encode_s_type(OPCODE_STORE, funct3, 8, 9, -40);
            assert_eq!(decode(raw), Ok(expected));
        }

        let invalid_store = encode_s_type(OPCODE_STORE, 0x3, 8, 9, 16);
        assert_eq!(
            decode(invalid_store),
            Err(CpuError::IllegalInstruction(invalid_store))
        );
    }

    // --- Тесты для Branch инструкций (0x63) ---

    #[test]
    fn test_decode_branch_instructions() {
        let cases = [
            (
                0x0,
                Instruction::Beq {
                    rs1: 10,
                    rs2: 11,
                    imm: 24,
                },
            ),
            (
                0x1,
                Instruction::Bne {
                    rs1: 10,
                    rs2: 11,
                    imm: 24,
                },
            ),
            (
                0x4,
                Instruction::Blt {
                    rs1: 10,
                    rs2: 11,
                    imm: 24,
                },
            ),
            (
                0x5,
                Instruction::Bge {
                    rs1: 10,
                    rs2: 11,
                    imm: 24,
                },
            ),
            (
                0x6,
                Instruction::Bltu {
                    rs1: 10,
                    rs2: 11,
                    imm: 24,
                },
            ),
            (
                0x7,
                Instruction::Bgeu {
                    rs1: 10,
                    rs2: 11,
                    imm: 24,
                },
            ),
        ];

        for (funct3, expected) in cases {
            let raw = encode_b_type(OPCODE_BRANCH, funct3, 10, 11, 24);
            assert_eq!(decode(raw), Ok(expected));
        }

        // Проверка сборки отрицательного B-immediate
        let raw_neg = encode_b_type(OPCODE_BRANCH, 0x0, 10, 11, -8);
        assert_eq!(
            decode(raw_neg),
            Ok(Instruction::Beq {
                rs1: 10,
                rs2: 11,
                imm: -8
            })
        );

        // Невалидный funct3 (0x2 или 0x3)
        let invalid_branch = encode_b_type(OPCODE_BRANCH, 0x2, 10, 11, 8);
        assert_eq!(
            decode(invalid_branch),
            Err(CpuError::IllegalInstruction(invalid_branch))
        );
    }

    // --- Тесты для JAL и U-type инструкций (LUI, AUIPC) ---

    #[test]
    fn test_decode_jal() {
        let raw_pos = encode_j_type(OPCODE_JAL, 1, 1024);
        assert_eq!(decode(raw_pos), Ok(Instruction::Jal { rd: 1, imm: 1024 }));

        let raw_neg = encode_j_type(OPCODE_JAL, 1, -2048);
        assert_eq!(decode(raw_neg), Ok(Instruction::Jal { rd: 1, imm: -2048 }));
    }

    #[test]
    fn test_decode_u_type_instructions() {
        let imm_val = 0x1234_5000_u32 as i32;

        let raw_lui = encode_u_type(OPCODE_LUI, 7, imm_val);
        assert_eq!(
            decode(raw_lui),
            Ok(Instruction::Lui {
                rd: 7,
                imm: imm_val
            })
        );

        let raw_auipc = encode_u_type(OPCODE_AUIPC, 8, imm_val);
        assert_eq!(
            decode(raw_auipc),
            Ok(Instruction::Auipc {
                rd: 8,
                imm: imm_val
            })
        );
    }

    // --- Тесты для Системных и Custom инструкций ---

    #[test]
    fn test_decode_system_instructions() {
        assert_eq!(decode(0x0000_0073), Ok(Instruction::Ecall));
        assert_eq!(decode(0x0010_0073), Ok(Instruction::Ebreak));

        // Любое другое значение для opcode 0x73 должно быть IllegalInstruction
        let invalid_sys = 0x0020_0073;
        assert_eq!(
            decode(invalid_sys),
            Err(CpuError::IllegalInstruction(invalid_sys))
        );
    }

    #[test]
    fn test_decode_custom_instructions() {
        let custom_0b = 0xDEAD_B00B;
        assert_eq!(
            decode(custom_0b),
            Ok(Instruction::Custom {
                opcode: 0x0B,
                raw: custom_0b
            })
        );

        let custom_2b = 0x1234_502B;
        assert_eq!(
            decode(custom_2b),
            Ok(Instruction::Custom {
                opcode: 0x2B,
                raw: custom_2b
            })
        );
    }
}
