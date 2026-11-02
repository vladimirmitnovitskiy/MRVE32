pub mod bus;
pub mod cpu;
pub mod devices;
pub mod elf;
pub mod machine;
pub mod plugin;
pub mod types;

pub use bus::{Bus, BusDevice, BusError};
pub use cpu::coprocessor::Coprocessor;
pub use cpu::csr::CsrError;
pub use cpu::disasm::{disassemble, is_compressed};
pub use cpu::instructions::Instruction;
pub use cpu::{Cpu, Exception, Interrupt, Trap, TrapCause};
pub use devices::clint::Clint;
pub use devices::finisher::TestFinisher;
pub use devices::ram::Ram;
pub use devices::uart::{Uart, UartIo};
pub use elf::{ElfError, ElfImage};
pub use machine::{Machine, RunResult, StopReason, TraceEvent, Tracer};
pub use plugin::{ForeignDevice, RawDevice};
pub use types::Reg;

#[cfg(feature = "python")]
pub mod python;

// Compiles and runs the Rust examples in README.md as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

#[cfg(doctest)]
#[doc = include_str!("../docs/devices.md")]
pub struct DevicesDoctests;
