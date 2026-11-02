//! A CPU together with its bus, with a run loop and breakpoints.

use std::collections::HashSet;

use crate::bus::{Bus, BusDevice};
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
    /// A device requested an exit with this code (see
    /// [`BusDevice::exit_request`]).
    Exit(u32),
}

/// One step as seen by a tracer (see [`Machine::set_tracer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEvent {
    /// `pc` before the step.
    pub pc: Reg,
    /// The instruction word executed (or attempted), see [`Cpu::last_fetch`].
    pub instruction: Option<u32>,
    /// The trap taken by the step, if any.
    pub trap: Option<Trap>,
}

/// Callback receiving every step; see [`Machine::set_tracer`].
pub type Tracer = Box<dyn FnMut(&TraceEvent)>;

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
    tracer: Option<Tracer>,
}

impl Machine {
    pub fn new(cpu: Cpu, bus: Bus) -> Self {
        Self {
            cpu,
            bus,
            breakpoints: HashSet::new(),
            tracer: None,
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

    /// Installs (or removes, with `None`) a callback invoked after every
    /// step, e.g. to print an instruction trace with
    /// [`disassemble`](crate::disassemble).
    pub fn set_tracer(&mut self, tracer: Option<Tracer>) {
        self.tracer = tracer;
    }

    /// Executes one step (see [`Cpu::step`]).
    pub fn step(&mut self) -> Option<Trap> {
        let pc = self.cpu.pc;
        let trap = self.cpu.step(&mut self.bus);
        if let Some(tracer) = &mut self.tracer {
            tracer(&TraceEvent {
                pc,
                instruction: self.cpu.last_fetch(),
                trap,
            });
        }
        trap
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
            let trap = self.step();
            if abort() {
                return RunResult {
                    steps,
                    reason: StopReason::Aborted,
                };
            }
            if let Some(code) = self.bus.exit_request() {
                return RunResult {
                    steps,
                    reason: StopReason::Exit(code),
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

    #[test]
    fn test_exit_request_stops_run() {
        use crate::TestFinisher;
        // lui a0, 0x100; lui a1, 0x5; addi a1, a1, 0x555; sw a1, 0(a0); nop
        let mut m = machine(&[0x00100537, 0x000055B7, 0x55558593, 0x00B52023, NOP, NOP]);
        m.bus
            .map_device(0x0010_0000, 0x1000, Box::new(TestFinisher::new()))
            .unwrap();
        let res = m.run(100, false);
        assert_eq!(
            res,
            RunResult {
                steps: 4,
                reason: StopReason::Exit(0)
            }
        );
    }

    #[test]
    fn test_tracer_sees_every_step() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let mut m = machine(&[NOP, 0xFFFF_FFFF]); // nop; illegal
        m.cpu.write_csr(crate::cpu::csr::MTVEC, 0x1000).unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        m.set_tracer(Some(Box::new(move |e: &TraceEvent| {
            sink.borrow_mut().push(*e)
        })));
        m.run(3, false);

        let events = events.borrow();
        assert_eq!(events.len(), 3);
        assert_eq!(
            (events[0].pc, events[0].instruction, events[0].trap),
            (0x1000, Some(NOP), None)
        );
        assert_eq!(events[1].pc, 0x1004);
        assert_eq!(events[1].instruction, Some(0xFFFF_FFFF));
        assert!(matches!(
            events[1].trap.map(|t| t.cause),
            Some(TrapCause::Exception(Exception::IllegalInstruction(_)))
        ));
        assert_eq!(events[2].pc, 0x1000, "handler at mtvec");
    }
}
