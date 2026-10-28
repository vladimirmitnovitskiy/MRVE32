use crate::bus::BusDevice;
use crate::cpu::CpuError;
use crate::types::Reg;

pub trait Coprocessor {
    fn execute(
        &mut self,
        opcode: u8,
        raw: u32,
        regs: &mut [Reg; 32],
        pc: &mut Reg,
        bus: &mut dyn BusDevice,
    ) -> Result<(), CpuError>;
}
