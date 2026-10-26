pub mod instructions;
pub mod decode;
pub mod coprocessor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    IllegalInstruction(u32)
}