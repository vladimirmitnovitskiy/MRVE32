use crate::cpu::{CpuError, instructions::Instruction};

pub fn decode(raw: u32) -> Result<Instruction, CpuError>{
    let opcode = (raw & 0x7F) as u8;
    let rd = ((raw >> 7) & 0x1F) as usize;
    let funct3 = ((raw >> 12) & 0x07) as u8;
    let rs1 = ((raw >> 15) & 0x1F) as usize;
    let rs2 = ((raw >> 20) & 0x1F) as usize;
    let funct7 = ((raw >> 25) & 0x7F) as u8;

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
            _ => Err(CpuError::IllegalInstruction(raw))
        },
        _ => Err(CpuError::IllegalInstruction(raw))
    }
} 

#[cfg(test)]
mod tests {
    use super::*;

    /// Вспомогательная функция для сборки 32-битного сырого R-type опкода RISC-V
    fn encode_r_type(opcode: u8, rd: usize, funct3: u8, rs1: usize, rs2: usize, funct7: u8) -> u32 {
        ((funct7 as u32 & 0x7F) << 25)
            | ((rs2 as u32 & 0x1F) << 20)
            | ((rs1 as u32 & 0x1F) << 15)
            | ((funct3 as u32 & 0x07) << 12)
            | ((rd as u32 & 0x1F) << 7)
            | (opcode as u32 & 0x7F)
    }

    const OPCODE_R_TYPE: u8 = 0x33;

    #[test]
    fn test_decode_all_valid_r_type_instructions() {
        let cases = [
            (0x0, 0x00, Instruction::Add { rd: 1, rs1: 2, rs2: 3 }),
            (0x0, 0x20, Instruction::Sub { rd: 1, rs1: 2, rs2: 3 }),
            (0x1, 0x00, Instruction::Sll { rd: 1, rs1: 2, rs2: 3 }),
            (0x2, 0x00, Instruction::Slt { rd: 1, rs1: 2, rs2: 3 }),
            (0x3, 0x00, Instruction::Sltu { rd: 1, rs1: 2, rs2: 3 }),
            (0x4, 0x00, Instruction::Xor { rd: 1, rs1: 2, rs2: 3 }),
            (0x5, 0x00, Instruction::Srl { rd: 1, rs1: 2, rs2: 3 }),
            (0x5, 0x20, Instruction::Sra { rd: 1, rs1: 2, rs2: 3 }),
            (0x6, 0x00, Instruction::Or { rd: 1, rs1: 2, rs2: 3 }),
            (0x7, 0x00, Instruction::And { rd: 1, rs1: 2, rs2: 3 }),
        ];

        for (funct3, funct7, expected) in cases {
            let raw = encode_r_type(OPCODE_R_TYPE, 1, funct3, 2, 3, funct7);
            assert_eq!(decode(raw), Ok(expected));
        }
    }

    #[test]
    fn test_register_bitfields_extraction() {
        // Проверяем крайние значения номеров регистров (от x0 до x31)
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
        // add x10, x11, x12  (rd=10, rs1=11, rs2=12, funct3=0, funct7=0, opcode=0x33)
        // 0000000 01100 01011 000 01010 0110011 -> 0x00C58533
        assert_eq!(
            decode(0x00C5_8533),
            Ok(Instruction::Add {
                rd: 10,
                rs1: 11,
                rs2: 12,
            })
        );

        // sub x1, x2, x3  (rd=1, rs1=2, rs2=3, funct3=0, funct7=0x20, opcode=0x33)
        // 0100000 00011 00010 000 00001 0110011 -> 0x403100B3
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
        // Неподдерживаемый opcode (например, I-type 0x13 или несуществующий 0x00)
        let raw_i_type = 0x0000_0013;
        assert_eq!(decode(raw_i_type), Err(CpuError::IllegalInstruction(raw_i_type)));

        let raw_zero = 0x0000_0000;
        assert_eq!(decode(raw_zero), Err(CpuError::IllegalInstruction(raw_zero)));
    }

    #[test]
    fn test_illegal_funct3_funct7_combinations() {
        // Валидный opcode 0x33, но невалидный funct7 для funct3=0 (например, 0x01 вместо 0x00 или 0x20)
        let invalid_funct7 = encode_r_type(OPCODE_R_TYPE, 1, 0x0, 2, 3, 0x01);
        assert_eq!(
            decode(invalid_funct7),
            Err(CpuError::IllegalInstruction(invalid_funct7))
        );

        // funct3 = 1 (Sll), но с funct7 = 0x20 (не существует в базовом RV32I)
        let invalid_sll = encode_r_type(OPCODE_R_TYPE, 1, 0x1, 2, 3, 0x20);
        assert_eq!(
            decode(invalid_sll),
            Err(CpuError::IllegalInstruction(invalid_sll))
        );
    }
}