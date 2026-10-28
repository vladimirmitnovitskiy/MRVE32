pub mod coprocessor;
pub mod decode;
pub mod instructions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    IllegalInstruction(u32),
}
