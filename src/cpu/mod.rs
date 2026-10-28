use crate::bus::BusError;

pub mod coprocessor;
pub mod decode;
pub mod instructions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    IllegalInstruction(u32),
    Bus(BusError),
}

impl From<BusError> for CpuError {
    fn from(err: BusError) -> Self {
        CpuError::Bus(err)
    }
}
