use crate::types::Reg;
    
// Machine Trap Setup
pub const MSTATUS: u16  = 0x300; // Machine status register
pub const MIE: u16      = 0x304; // Machine interrupt enable
pub const MTVEC: u16    = 0x305; // Machine trap-handler base address

// Machine Trap Handling
pub const MSCRATCH: u16 = 0x340; // Scratch register for trap handlers
pub const MEPC: u16     = 0x341; // Machine exception program counter
pub const MCAUSE: u16   = 0x342; // Machine trap cause
pub const MTVAL: u16    = 0x343; // Machine bad address or instruction
pub const MIP: u16      = 0x344; // Machine interrupt pending

// Machine Information
pub const MHARTID: u16  = 0xF14; 

#[derive(Debug, Clone, Copy, Default)]
pub struct CsrFile {
    pub mstatus: Reg,
    pub mie: Reg,
    pub mtvec: Reg,
    pub mscratch: Reg,
    pub mepc: Reg,
    pub mcause: Reg,
    pub mtval: Reg,
    pub mip: Reg,
}

impl CsrFile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&self, csr: u16) -> Reg {
        match csr {
            MSTATUS => self.mstatus,
            MIE => self.mie,
            MTVEC => self.mtvec,
            MSCRATCH => self.mscratch,
            MEPC => self.mepc,
            MCAUSE => self.mcause,
            MTVAL => self.mtval,
            MIP => self.mip,
            MHARTID => 0, // hart id = 0
            _ => 0,       // чтение неподдерживаемых CSR возвращает 0
        }
    }

    pub fn write(&mut self, csr: u16, val: Reg) {
        match csr {
            MSTATUS => self.mstatus = val,
            MIE => self.mie = val,
            MTVEC => self.mtvec = val,
            MSCRATCH => self.mscratch = val,
            MEPC => self.mepc = val,
            MCAUSE => self.mcause = val,
            MTVAL => self.mtval = val,
            MIP => self.mip = val,
            _ => {} // запись в неподдерживаемые CSR (satp, pmpcfg и др.) безопасно игнорируется
        }
    }
}
