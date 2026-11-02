use crate::bus::BusDevice;
use crate::cpu::Exception;
use crate::types::Reg;

/// Extension point for custom instructions in the `custom-0` (`0x0B`) and
/// `custom-1` (`0x2B`) opcode spaces.
///
/// `regs` and `next_pc` are scratch copies: they are committed to the CPU only
/// if `execute` returns `Ok`, so returning an [`Exception`] leaves the
/// register file untouched and raises the corresponding trap. Writes to `x0`
/// are discarded. Bus side effects cannot be rolled back.
pub trait Coprocessor {
    fn execute(
        &mut self,
        opcode: u8,
        raw: u32,
        regs: &mut [Reg; 32],
        next_pc: &mut Reg,
        bus: &mut dyn BusDevice,
    ) -> Result<(), Exception>;
}
