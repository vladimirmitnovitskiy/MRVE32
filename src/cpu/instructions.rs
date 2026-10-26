#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    // arithmetic registers
    Add {rd: usize, rs1: usize, rs2: usize},
    Sub {rd: usize, rs1: usize, rs2: usize},
    Sll {rd: usize, rs1: usize, rs2: usize},
    Slt {rd: usize, rs1: usize, rs2: usize},
    Sltu {rd: usize, rs1: usize, rs2: usize},
    Xor {rd: usize, rs1: usize, rs2: usize},
    Srl {rd: usize, rs1: usize, rs2: usize},
    Sra {rd: usize, rs1: usize, rs2: usize},
    Or {rd: usize, rs1: usize, rs2: usize},
    And {rd: usize, rs1: usize, rs2: usize},

    // arithmetic with const
    Addi {rd: usize, rs1: usize, imm: i32},
    Slti {rd: usize, rs1: usize, imm: i32},
    Sltiu {rd: usize, rs1: usize, imm: i32},
    Xori {rd: usize, rs1: usize, imm: i32},
    Ori {rd: usize, rs1: usize, imm: i32},
    Andi {rd: usize, rs1: usize, imm: i32},
    Slli {rd: usize, rs1: usize, imm: i32},
    Srli {rd: usize, rs1: usize, imm: i32},
    Srai {rd: usize, rs1: usize, imm: i32},

    // read from memory
    Lb {rd: usize, rs1: usize, imm: i32},
    Lh {rd: usize, rs1: usize, imm: i32},
    Lw {rd: usize, rs1: usize, imm: i32},
    Lbu {rd: usize, rs1: usize, imm: i32},
    Lhu {rd: usize, rs1: usize, imm: i32},

    // write in memory
    Sb {rs1: usize, rs2: usize, imm: i32},
    Sh {rs1: usize, rs2: usize, imm: i32},
    Sw {rs1: usize, rs2: usize, imm: i32},

    // branches
    Beq {rs1: usize, rs2: usize, imm: i32},
    Bne {rs1: usize, rs2: usize, imm: i32},
    Blt {rs1: usize, rs2: usize, imm: i32},
    Bge {rs1: usize, rs2: usize, imm: i32},
    Bltu {rs1: usize, rs2: usize, imm: i32},
    Bgeu {rs1: usize, rs2: usize, imm: i32},

    // Elder const
    Lui {rd: usize, imm: i32},
    Auipc {rd: usize, imm: i32},

    // Jumps
    Jal {rd: usize, imm: i32},
    Jalr {rd: usize, rs1: usize, imm: i32},

    // system and coprocessor
    Ecall,
    Ebreak,
    Custom {opcode: u8, raw: u32}
}