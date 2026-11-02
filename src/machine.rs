//! A CPU together with its bus, with a run loop and breakpoints.

use std::collections::HashSet;

use crate::bus::Bus;
use crate::cpu::{Cpu, Trap};
use crate::elf::{ElfError, ElfImage};
use crate::types::Reg;

/// Why [`Machine::run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// `max_steps` steps were executed.
    MaxSteps,
    /// `pc` reached a breakpoint (the instruction there has not executed).
    Breakpoint(Reg),
    /// A trap was taken and `stop_on_trap` was set (`pc` is the handler).
    Trap(Trap),
    /// The hart waits in `wfi` with `mie == 0`, so it can never wake up.
    Idle,
    /// The `abort` callback of [`Machine::run_until`] returned true.
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunResult {
    /// Steps (clock cycles) executed by this call.
    pub steps: u64,
    pub reason: StopReason,
}

pub struct Machine {
    pub cpu: Cpu,
    pub bus: Bus,
    breakpoints: HashSet<Reg>,
}

impl Machine {
    pub fn new(cpu: Cpu, bus: Bus) -> Self {
        Self {
            cpu,
            bus,
            breakpoints: HashSet::new(),
        }
    }

    /// Loads an ELF image onto the bus and sets `pc` to its entry point.
    pub fn load_elf(&mut self, image: &ElfImage) -> Result<(), ElfError> {
        image.load(&mut self.bus)?;
        self.cpu.pc = image.entry;
        Ok(())
    }

    pub fn add_breakpoint(&mut self, pc: Reg) {
        self.breakpoints.insert(pc);
    }

    pub fn remove_breakpoint(&mut self, pc: Reg) -> bool {
        self.breakpoints.remove(&pc)
    }

    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    pub fn breakpoints(&self) -> impl Iterator<Item = Reg> + '_ {
        self.breakpoints.iter().copied()
    }

    /// Executes one step (see [`Cpu::step`]).
    pub fn step(&mut self) -> Option<Trap> {
        self.cpu.step(&mut self.bus)
    }

    /// Runs up to `max_steps` steps.
    ///
    /// Breakpoints are checked after every step, so a run that starts on a
    /// breakpoint executes that instruction first (continuing works without
    /// removing the breakpoint).
    pub fn run(&mut self, max_steps: u64, stop_on_trap: bool) -> RunResult {
        self.run_until(max_steps, stop_on_trap, || false)
    }

    /// Like [`run`](Self::run), but also stops with [`StopReason::Aborted`]
    /// when `abort` returns true after a step (checked before the other
    /// conditions), e.g. when a device reported an error out of band.
    pub fn run_until(
        &mut self,
        max_steps: u64,
        stop_on_trap: bool,
        mut abort: impl FnMut() -> bool,
    ) -> RunResult {
        for steps in 1..=max_steps {
            let trap = self.cpu.step(&mut self.bus);
            if abort() {
                return RunResult {
                    steps,
                    reason: StopReason::Aborted,
                };
            }
            if let Some(trap) = trap.filter(|_| stop_on_trap) {
                return RunResult {
                    steps,
                    reason: StopReason::Trap(trap),
                };
            }
            if self.breakpoints.contains(&self.cpu.pc) {
                return RunResult {
                    steps,
                    reason: StopReason::Breakpoint(self.cpu.pc),
                };
            }
            if self.cpu.is_waiting() && self.cpu.csrs.mie == 0 {
                return RunResult {
                    steps,
                    reason: StopReason::Idle,
                };
            }
        }
        RunResult {
            steps: max_steps,
            reason: StopReason::MaxSteps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BusDevice, Exception, Ram, TrapCause};

    fn machine(program: &[u32]) -> Machine {
        let mut ram = Ram::new(4096);
        for (i, &w) in program.iter().enumerate() {
            ram.write_u32(4 * i as Reg, w).unwrap();
        }
        let mut bus = Bus::new();
        bus.map_device(0x1000, 4096, Box::new(ram)).unwrap();
        Machine::new(Cpu::new(0x1000), bus)
    }

    const NOP: u32 = 0x0000_0013;

    #[test]
    fn test_run_until_max_steps() {
        let mut m = machine(&[NOP; 16]);
        let res = m.run(10, false);
        assert_eq!(
            res,
            RunResult {
                steps: 10,
                reason: StopReason::MaxSteps
            }
        );
        assert_eq!(m.cpu.pc, 0x1028);
    }

    #[test]
    fn test_breakpoint_stops_before_instruction_and_continue_works() {
        let mut m = machine(&[NOP; 16]);
        m.add_breakpoint(0x1008);

        let res = m.run(100, false);
        assert_eq!(
            res,
            RunResult {
                steps: 2,
                reason: StopReason::Breakpoint(0x1008)
            }
        );
        assert_eq!(m.cpu.pc, 0x1008);

        // Продолжение со стоящей точки останова
        let res = m.run(3, false);
        assert_eq!(res.reason, StopReason::MaxSteps);
        assert_eq!(m.cpu.pc, 0x1014);

        assert!(m.remove_breakpoint(0x1008));
        assert!(!m.remove_breakpoint(0x1008));
    }

    #[test]
    fn test_stop_on_trap() {
        let mut m = machine(&[NOP, 0xFFFF_FFFF, NOP]);
        m.cpu.write_csr(crate::cpu::csr::MTVEC, 0x1100).unwrap();

        let res = m.run(10, true);
        let trap = Trap {
            cause: TrapCause::Exception(Exception::IllegalInstruction(0xFFFF_FFFF)),
            epc: 0x1004,
        };
        assert_eq!(
            res,
            RunResult {
                steps: 2,
                reason: StopReason::Trap(trap)
            }
        );
        assert_eq!(m.cpu.pc, 0x1100);
    }

    #[test]
    fn test_traps_do_not_stop_without_flag() {
        let mut m = machine(&[0xFFFF_FFFF]);
        m.cpu.write_csr(crate::cpu::csr::MTVEC, 0x1000).unwrap();
        assert_eq!(m.run(5, false).reason, StopReason::MaxSteps);
    }

    #[test]
    fn test_idle_when_waiting_with_no_enabled_interrupts() {
        let mut m = machine(&[NOP, 0x1050_0073, NOP]); // nop; wfi
        let res = m.run(100, false);
        assert_eq!(
            res,
            RunResult {
                steps: 2,
                reason: StopReason::Idle
            }
        );
    }

    #[test]
    fn test_run_until_aborts_after_step() {
        let mut m = machine(&[NOP; 16]);
        let mut calls = 0;
        let res = m.run_until(100, false, || {
            calls += 1;
            calls == 3
        });
        assert_eq!(
            res,
            RunResult {
                steps: 3,
                reason: StopReason::Aborted
            }
        );
    }
}
