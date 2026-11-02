pub mod bus;
pub mod cpu;
pub mod devices;
pub mod types;

pub use bus::{Bus, BusDevice, BusError};
pub use cpu::coprocessor::Coprocessor;
pub use cpu::csr::CsrError;
pub use cpu::instructions::Instruction;
pub use cpu::{Cpu, Exception, Interrupt, Trap, TrapCause};
pub use devices::clint::Clint;
pub use devices::ram::Ram;
pub use devices::snn::{SnnAccelerator, SnnConfig};
pub use types::Reg;

#[cfg(feature = "python")]
pub mod python;

// Compiles and runs the Rust examples in README.md as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;
