use std::fmt;

use crate::types::Reg;

/// A synchronous exception defined by the RISC-V privileged specification.
///
/// The payload is the value written to `mtval` (a faulting address or the
/// raw instruction word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Exception {
    /// Taken jump/branch (or fetch) to an address that is not 4-byte aligned.
    InstructionAddressMisaligned(Reg),
    /// Instruction fetch from an unmapped address.
    InstructionAccessFault(Reg),
    /// Undecodable instruction, unimplemented CSR or write to a read-only CSR.
    IllegalInstruction(u32),
    /// `ebreak`; the payload is the address of the instruction.
    Breakpoint(Reg),
    LoadAddressMisaligned(Reg),
    LoadAccessFault(Reg),
    StoreAddressMisaligned(Reg),
    StoreAccessFault(Reg),
    /// `ecall` executed in Machine mode.
    EnvironmentCallFromM,
}

impl Exception {
    /// Exception code written to `mcause` (interrupt bit clear).
    pub fn code(&self) -> Reg {
        match self {
            Exception::InstructionAddressMisaligned(_) => 0,
            Exception::InstructionAccessFault(_) => 1,
            Exception::IllegalInstruction(_) => 2,
            Exception::Breakpoint(_) => 3,
            Exception::LoadAddressMisaligned(_) => 4,
            Exception::LoadAccessFault(_) => 5,
            Exception::StoreAddressMisaligned(_) => 6,
            Exception::StoreAccessFault(_) => 7,
            Exception::EnvironmentCallFromM => 11,
        }
    }

    /// Value written to `mtval`.
    pub fn tval(&self) -> Reg {
        match *self {
            Exception::InstructionAddressMisaligned(v)
            | Exception::InstructionAccessFault(v)
            | Exception::IllegalInstruction(v)
            | Exception::Breakpoint(v)
            | Exception::LoadAddressMisaligned(v)
            | Exception::LoadAccessFault(v)
            | Exception::StoreAddressMisaligned(v)
            | Exception::StoreAccessFault(v) => v,
            Exception::EnvironmentCallFromM => 0,
        }
    }
}

impl fmt::Display for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Exception::InstructionAddressMisaligned(a) => {
                write!(f, "instruction address misaligned: 0x{a:08X}")
            }
            Exception::InstructionAccessFault(a) => {
                write!(f, "instruction access fault: 0x{a:08X}")
            }
            Exception::IllegalInstruction(raw) => write!(f, "illegal instruction: 0x{raw:08X}"),
            Exception::Breakpoint(a) => write!(f, "breakpoint at 0x{a:08X}"),
            Exception::LoadAddressMisaligned(a) => write!(f, "load address misaligned: 0x{a:08X}"),
            Exception::LoadAccessFault(a) => write!(f, "load access fault: 0x{a:08X}"),
            Exception::StoreAddressMisaligned(a) => {
                write!(f, "store address misaligned: 0x{a:08X}")
            }
            Exception::StoreAccessFault(a) => write!(f, "store access fault: 0x{a:08X}"),
            Exception::EnvironmentCallFromM => write!(f, "environment call from M-mode"),
        }
    }
}

impl std::error::Error for Exception {}

/// A trap that has been taken by the CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trap {
    pub exception: Exception,
    /// Address of the instruction that raised the exception (written to `mepc`).
    pub epc: Reg,
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (pc=0x{:08X})", self.exception, self.epc)
    }
}

impl std::error::Error for Trap {}
