/// Operation of an RV32A atomic memory operation (`amo<op>.w`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmoOp {
    Swap,
    Add,
    Xor,
    And,
    Or,
    Min,
    Max,
    Minu,
    Maxu,
}

impl AmoOp {
    /// The value written to memory, from the loaded value and `rs2`.
    pub fn apply(self, old: u32, src: u32) -> u32 {
        match self {
            AmoOp::Swap => src,
            AmoOp::Add => old.wrapping_add(src),
            AmoOp::Xor => old ^ src,
            AmoOp::And => old & src,
            AmoOp::Or => old | src,
            AmoOp::Min => (old as i32).min(src as i32) as u32,
            AmoOp::Max => (old as i32).max(src as i32) as u32,
            AmoOp::Minu => old.min(src),
            AmoOp::Maxu => old.max(src),
        }
    }

    pub fn mnemonic(self) -> &'static str {
        match self {
            AmoOp::Swap => "amoswap.w",
            AmoOp::Add => "amoadd.w",
            AmoOp::Xor => "amoxor.w",
            AmoOp::And => "amoand.w",
            AmoOp::Or => "amoor.w",
            AmoOp::Min => "amomin.w",
            AmoOp::Max => "amomax.w",
            AmoOp::Minu => "amominu.w",
            AmoOp::Maxu => "amomaxu.w",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    // arithmetic registers. R-type
    Add {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Sub {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Sll {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Slt {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Sltu {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Xor {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Srl {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Sra {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Or {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    And {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    // RV32M Extension
    Mul {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Mulh {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Mulhsu {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Mulhu {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Div {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Divu {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Rem {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },
    Remu {
        rd: usize,
        rs1: usize,
        rs2: usize,
    },

    // arithmetic with const. I-type
    Addi {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Slti {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Sltiu {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Xori {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Ori {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Andi {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Slli {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Srli {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Srai {
        rd: usize,
        rs1: usize,
        imm: i32,
    },

    // read from memory
    Lb {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Lh {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Lw {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Lbu {
        rd: usize,
        rs1: usize,
        imm: i32,
    },
    Lhu {
        rd: usize,
        rs1: usize,
        imm: i32,
    },

    // write in memory
    Sb {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },
    Sh {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },
    Sw {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },

    // branches
    Beq {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },
    Bne {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },
    Blt {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },
    Bge {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },
    Bltu {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },
    Bgeu {
        rs1: usize,
        rs2: usize,
        imm: i32,
    },

    // Elder const
    Lui {
        rd: usize,
        imm: i32,
    },
    Auipc {
        rd: usize,
        imm: i32,
    },

    // Jumps
    Jal {
        rd: usize,
        imm: i32,
    },
    Jalr {
        rd: usize,
        rs1: usize,
        imm: i32,
    },

    // system and coprocessor
    Ecall,
    Ebreak,
    Custom {
        opcode: u8,
        raw: u32,
    },
    /// `pred`/`succ` are the IORW bit sets; `fm = 0b1000` with `rw, rw` is
    /// `fence.tso`. Reserved encodings execute as a normal fence.
    Fence {
        fm: u8,
        pred: u8,
        succ: u8,
    },
    FenceI,
    Mret,
    Wfi,
    // RV32A (aq/rl have no effect on a single in-order hart)
    LrW {
        rd: usize,
        rs1: usize,
        aq: bool,
        rl: bool,
    },
    ScW {
        rd: usize,
        rs1: usize,
        rs2: usize,
        aq: bool,
        rl: bool,
    },
    Amo {
        op: AmoOp,
        rd: usize,
        rs1: usize,
        rs2: usize,
        aq: bool,
        rl: bool,
    },
    // Zicsr
    Csrrw {
        rd: usize,
        rs1: usize,
        csr: u16,
    },
    Csrrs {
        rd: usize,
        rs1: usize,
        csr: u16,
    },
    Csrrc {
        rd: usize,
        rs1: usize,
        csr: u16,
    },
    Csrrwi {
        rd: usize,
        uimm: u32,
        csr: u16,
    },
    Csrrsi {
        rd: usize,
        uimm: u32,
        csr: u16,
    },
    Csrrci {
        rd: usize,
        uimm: u32,
        csr: u16,
    },
}
