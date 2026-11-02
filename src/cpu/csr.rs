use std::fmt;

use crate::types::Reg;

// Machine Information Registers (read-only)
pub const MVENDORID: u16 = 0xF11;
pub const MARCHID: u16 = 0xF12;
pub const MIMPID: u16 = 0xF13;
pub const MHARTID: u16 = 0xF14;
pub const MCONFIGPTR: u16 = 0xF15;

// Machine Trap Setup
pub const MSTATUS: u16 = 0x300; // Machine status register
pub const MISA: u16 = 0x301; // ISA and extensions
pub const MIE: u16 = 0x304; // Machine interrupt enable
pub const MTVEC: u16 = 0x305; // Machine trap-handler base address
pub const MSTATUSH: u16 = 0x310; // Upper half of mstatus (RV32 only)

// Machine Trap Handling
pub const MSCRATCH: u16 = 0x340; // Scratch register for trap handlers
pub const MEPC: u16 = 0x341; // Machine exception program counter
pub const MCAUSE: u16 = 0x342; // Machine trap cause
pub const MTVAL: u16 = 0x343; // Machine bad address or instruction
pub const MIP: u16 = 0x344; // Machine interrupt pending

// Machine Counter/Timers
pub const MCOUNTINHIBIT: u16 = 0x320; // Machine counter-inhibit register
pub const MCYCLE: u16 = 0xB00;
pub const MINSTRET: u16 = 0xB02;
pub const MCYCLEH: u16 = 0xB80;
pub const MINSTRETH: u16 = 0xB82;
// mhpmcounter3..31 (+h) and mhpmevent3..31 are implemented as hardwired zero.
const MHPMCOUNTER3: u16 = 0xB03;
const MHPMCOUNTER31: u16 = 0xB1F;
const MHPMCOUNTER3H: u16 = 0xB83;
const MHPMCOUNTER31H: u16 = 0xB9F;
const MHPMEVENT3: u16 = 0x323;
const MHPMEVENT31: u16 = 0x33F;

// Unprivileged Counter/Timers (Zicntr, read-only shadows)
pub const CYCLE: u16 = 0xC00;
pub const INSTRET: u16 = 0xC02;
pub const CYCLEH: u16 = 0xC80;
pub const INSTRETH: u16 = 0xC82;
/// `time`/`timeh` are not implemented (they trap; read `mtime` from the
/// CLINT); the numbers are used by the disassembler.
pub const TIME: u16 = 0xC01;
pub const TIMEH: u16 = 0xC81;

pub const MSTATUS_MIE: Reg = 1 << 3; // Machine Interrupt Enable
pub const MSTATUS_MPIE: Reg = 1 << 7; // Machine Previous Interrupt Enable
pub const MSTATUS_MPP: Reg = 0b11 << 11; // Machine Previous Privilege (hardwired to M)

pub const MIP_MSIP: Reg = 1 << 3; // Machine software interrupt
pub const MIP_MTIP: Reg = 1 << 7; // Machine timer interrupt
pub const MIP_MEIP: Reg = 1 << 11; // Machine external interrupt

pub const MCOUNTINHIBIT_CY: Reg = 1 << 0; // Inhibit mcycle
pub const MCOUNTINHIBIT_IR: Reg = 1 << 2; // Inhibit minstret

/// `misa`: MXL=1 (XLEN=32), extensions I and M.
/// `misa` without C: MXL = 32, A, I, M.
pub const MISA_VALUE: Reg = (1 << 30) | 1 | (1 << (b'I' - b'A')) | (1 << (b'M' - b'A'));
/// `misa.C`.
pub const MISA_C: Reg = 1 << (b'C' - b'A');

const MSTATUS_WRITABLE: Reg = MSTATUS_MIE | MSTATUS_MPIE;
const MIE_WRITABLE: Reg = MIP_MSIP | MIP_MTIP | MIP_MEIP;

/// Why a CSR access was rejected. Executed by an instruction, either case
/// raises an illegal-instruction exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsrError {
    /// The CSR does not exist on this hart.
    Unimplemented(u16),
    /// The CSR is read-only (address bits `[11:10] == 0b11`).
    ReadOnly(u16),
}

impl fmt::Display for CsrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CsrError::Unimplemented(csr) => write!(f, "CSR 0x{csr:03X} is not implemented"),
            CsrError::ReadOnly(csr) => write!(f, "CSR 0x{csr:03X} is read-only"),
        }
    }
}

impl std::error::Error for CsrError {}

/// Machine-mode CSRs.
///
/// [`CsrFile::read`]/[`CsrFile::write`] implement software-visible access
/// (WARL masking, read-only checks). The public fields hold the raw
/// architectural state. `mip` is recomputed by the CPU from the bus
/// interrupt lines at the start of every step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsrFile {
    pub mstatus: Reg,
    pub mie: Reg,
    pub mtvec: Reg,
    pub mscratch: Reg,
    pub mepc: Reg,
    pub mcause: Reg,
    pub mtval: Reg,
    pub mip: Reg,
    pub mcountinhibit: Reg,
    pub mcycle: u64,
    pub minstret: u64,
    /// Set when software writes mcycle/minstret, which suppresses the
    /// automatic increment for that instruction.
    pub(crate) cycle_written: bool,
    pub(crate) instret_written: bool,
    /// C extension enabled: IALIGN = 16 instead of 32.
    pub(crate) compressed: bool,
}

impl Default for CsrFile {
    fn default() -> Self {
        Self {
            mstatus: MSTATUS_MPP,
            mie: 0,
            mtvec: 0,
            mscratch: 0,
            mepc: 0,
            mcause: 0,
            mtval: 0,
            mip: 0,
            mcountinhibit: 0,
            mcycle: 0,
            minstret: 0,
            cycle_written: false,
            instret_written: false,
            compressed: true,
        }
    }
}

fn set_lo(counter: &mut u64, val: Reg) {
    *counter = (*counter & !0xFFFF_FFFF) | val as u64;
}

fn set_hi(counter: &mut u64, val: Reg) {
    *counter = (*counter & 0xFFFF_FFFF) | ((val as u64) << 32);
}

impl CsrFile {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the CSR address encodes a read-only register.
    pub fn is_read_only(csr: u16) -> bool {
        (csr >> 10) & 0b11 == 0b11
    }

    pub fn read(&self, csr: u16) -> Result<Reg, CsrError> {
        Ok(match csr {
            MSTATUS => self.mstatus,
            MISA => MISA_VALUE | if self.compressed { MISA_C } else { 0 },
            MIE => self.mie,
            MTVEC => self.mtvec,
            MSTATUSH => 0,
            MSCRATCH => self.mscratch,
            // With IALIGN = 32, mepc[1] reads as zero (but is kept).
            MEPC if self.compressed => self.mepc,
            MEPC => self.mepc & !0b11,
            MCAUSE => self.mcause,
            MTVAL => self.mtval,
            MIP => self.mip,
            MVENDORID | MARCHID | MIMPID | MHARTID | MCONFIGPTR => 0,
            MCOUNTINHIBIT => self.mcountinhibit,
            MCYCLE | CYCLE => self.mcycle as Reg,
            MCYCLEH | CYCLEH => (self.mcycle >> 32) as Reg,
            MINSTRET | INSTRET => self.minstret as Reg,
            MINSTRETH | INSTRETH => (self.minstret >> 32) as Reg,
            MHPMCOUNTER3..=MHPMCOUNTER31
            | MHPMCOUNTER3H..=MHPMCOUNTER31H
            | MHPMEVENT3..=MHPMEVENT31 => 0,
            _ => return Err(CsrError::Unimplemented(csr)),
        })
    }

    pub fn write(&mut self, csr: u16, val: Reg) -> Result<(), CsrError> {
        self.read(csr)?;
        if Self::is_read_only(csr) {
            return Err(CsrError::ReadOnly(csr));
        }
        match csr {
            // Only MIE/MPIE are writable; MPP is hardwired to M (no U/S modes).
            MSTATUS => self.mstatus = (val & MSTATUS_WRITABLE) | MSTATUS_MPP,
            MIE => self.mie = val & MIE_WRITABLE,
            // MODE is WARL: 0 = direct, 1 = vectored; reserved values map onto these.
            MTVEC => self.mtvec = val & !0b10,
            MSCRATCH => self.mscratch = val,
            // mepc[0] is always zero (IALIGN >= 16).
            MEPC => self.mepc = val & !1,
            MCAUSE => self.mcause = val,
            MTVAL => self.mtval = val,
            // Only CY and IR are writable (TM is read-only zero, no hpm counters).
            MCOUNTINHIBIT => self.mcountinhibit = val & (MCOUNTINHIBIT_CY | MCOUNTINHIBIT_IR),
            MCYCLE => {
                set_lo(&mut self.mcycle, val);
                self.cycle_written = true;
            }
            MCYCLEH => {
                set_hi(&mut self.mcycle, val);
                self.cycle_written = true;
            }
            MINSTRET => {
                set_lo(&mut self.minstret, val);
                self.instret_written = true;
            }
            MINSTRETH => {
                set_hi(&mut self.minstret, val);
                self.instret_written = true;
            }
            // misa, mstatush and mhpm* are fixed; MSIP/MTIP/MEIP in mip are driven by
            // the platform, not by software.
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unimplemented_csr_is_rejected() {
        let mut csrs = CsrFile::new();
        // satp, medeleg, pmpcfg0 do not exist on an M-only hart
        for csr in [0x180, 0x302, 0x3A0] {
            assert_eq!(csrs.read(csr), Err(CsrError::Unimplemented(csr)));
            assert_eq!(csrs.write(csr, 0), Err(CsrError::Unimplemented(csr)));
        }
    }

    #[test]
    fn test_read_only_csrs() {
        let mut csrs = CsrFile::new();
        for csr in [MVENDORID, MARCHID, MIMPID, MHARTID, MCONFIGPTR] {
            assert_eq!(csrs.read(csr), Ok(0));
            assert_eq!(csrs.write(csr, 1), Err(CsrError::ReadOnly(csr)));
        }
    }

    #[test]
    fn test_misa_reports_extensions_and_ignores_writes() {
        let mut csrs = CsrFile::new();
        assert_eq!(csrs.read(MISA), Ok(0x4000_1105), "RV32IMAC");
        assert_eq!(csrs.write(MISA, 0), Ok(()));
        assert_eq!(csrs.read(MISA), Ok(0x4000_1105));
        csrs.compressed = false;
        assert_eq!(csrs.read(MISA), Ok(0x4000_1101), "RV32IMA");
    }

    #[test]
    fn test_counters_64bit_halves_and_shadows() {
        let mut csrs = CsrFile::new();
        csrs.write(MCYCLE, 0xDEAD_BEEF).unwrap();
        csrs.write(MCYCLEH, 0x1234_5678).unwrap();
        assert_eq!(csrs.mcycle, 0x1234_5678_DEAD_BEEF);
        assert_eq!(csrs.read(CYCLE), Ok(0xDEAD_BEEF));
        assert_eq!(csrs.read(CYCLEH), Ok(0x1234_5678));

        csrs.write(MINSTRETH, 1).unwrap();
        csrs.write(MINSTRET, 2).unwrap();
        assert_eq!(csrs.minstret, 0x1_0000_0002);
        assert_eq!(csrs.read(INSTRET), Ok(2));
        assert_eq!(csrs.read(INSTRETH), Ok(1));

        // Unprivileged shadows are read-only
        assert_eq!(csrs.write(CYCLE, 0), Err(CsrError::ReadOnly(CYCLE)));
        assert_eq!(csrs.write(INSTRETH, 0), Err(CsrError::ReadOnly(INSTRETH)));
    }

    #[test]
    fn test_hpm_counters_are_hardwired_zero() {
        let mut csrs = CsrFile::new();
        for csr in [0xB03, 0xB1F, 0xB83, 0xB9F, 0x323, 0x33F] {
            assert_eq!(csrs.write(csr, 0xFFFF_FFFF), Ok(()));
            assert_eq!(csrs.read(csr), Ok(0));
        }
        // mcountinhibit: only CY and IR are writable
        csrs.write(MCOUNTINHIBIT, 0xFFFF_FFFF).unwrap();
        assert_eq!(csrs.read(MCOUNTINHIBIT), Ok(0b101));
    }

    #[test]
    fn test_warl_masks() {
        let mut csrs = CsrFile::new();

        csrs.write(MSTATUS, 0xFFFF_FFFF).unwrap();
        assert_eq!(
            csrs.read(MSTATUS),
            Ok(MSTATUS_MIE | MSTATUS_MPIE | MSTATUS_MPP)
        );
        csrs.write(MSTATUS, 0).unwrap();
        assert_eq!(csrs.read(MSTATUS), Ok(MSTATUS_MPP));

        // IALIGN = 16: bit 0 is always zero.
        csrs.write(MEPC, 0x1003).unwrap();
        assert_eq!(csrs.read(MEPC), Ok(0x1002));
        // IALIGN = 32: bit 1 reads as zero but is kept.
        csrs.compressed = false;
        assert_eq!(csrs.read(MEPC), Ok(0x1000));
        csrs.compressed = true;
        assert_eq!(csrs.read(MEPC), Ok(0x1002));

        csrs.write(MTVEC, 0x2001).unwrap();
        assert_eq!(csrs.read(MTVEC), Ok(0x2001)); // vectored
        csrs.write(MTVEC, 0x2002).unwrap();
        assert_eq!(csrs.read(MTVEC), Ok(0x2000)); // reserved mode -> direct

        csrs.write(MIE, 0xFFFF_FFFF).unwrap();
        assert_eq!(csrs.read(MIE), Ok(MIP_MSIP | MIP_MTIP | MIP_MEIP));

        csrs.write(MIP, 0xFFFF_FFFF).unwrap();
        assert_eq!(csrs.read(MIP), Ok(0));

        csrs.write(MSTATUSH, 0xFFFF_FFFF).unwrap();
        assert_eq!(csrs.read(MSTATUSH), Ok(0));
    }
}
