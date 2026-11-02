//! HTIF `tohost` exit and console (as in Spike and the Sail model), for test
//! suites such as riscv-tests and the RISC-V Architectural Certification
//! Tests.
//!
//! [`Htif`] wraps the memory that holds the 64-bit `tohost` variable (usually
//! RAM, at the address of the ELF symbol `tohost`) and passes every access
//! through. A command is complete when the upper word (`tohost + 4`) is
//! written with a 32-bit store, the lower word having been written first:
//!
//! | device (bits 63:56) | command (55:48) | payload (47:0) | effect |
//! |---|---|---|---|
//! | 0 | 0 | `(code << 1) \| 1` | exit with `code` (0 = pass) |
//! | 1 | 1 | byte | print the byte (console output) |
//!
//! Other commands (syscall proxying, console input) are ignored. Like the
//! test finisher, an exit request is sticky.

use crate::bus::{BusDevice, BusError};
use crate::types::Reg;

/// Receives HTIF console output.
pub type HtifSink = Box<dyn FnMut(u8)>;

pub struct Htif<D> {
    inner: D,
    /// Offset of `tohost` inside `inner`.
    tohost: Reg,
    low: u32,
    exit: Option<u32>,
    sink: Option<HtifSink>,
}

impl<D: BusDevice> Htif<D> {
    /// Watches the 64-bit `tohost` at offset `tohost` (8-byte aligned in
    /// practice; must be 4-byte aligned) of `inner`.
    pub fn new(inner: D, tohost: Reg) -> Self {
        Self {
            inner,
            tohost,
            low: 0,
            exit: None,
            sink: None,
        }
    }

    /// Sends console output (device 1, command 1) to `sink`; without one
    /// it is discarded.
    pub fn set_sink(&mut self, sink: Option<HtifSink>) {
        self.sink = sink;
    }

    pub fn inner(&self) -> &D {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut D {
        &mut self.inner
    }

    fn command(&mut self, high: u32) {
        let device = high >> 24;
        let cmd = (high >> 16) & 0xFF;
        match (device, cmd) {
            (0, 0) if self.low & 1 == 1 && self.exit.is_none() => {
                // Bits 47:33 of the code are ignored (exit codes are u32).
                self.exit = Some(self.low >> 1 | (high & 0xFFFF) << 31);
            }
            (1, 1) => {
                if let Some(sink) = &mut self.sink {
                    sink(self.low as u8);
                }
            }
            _ => {}
        }
    }
}

impl<D: BusDevice> BusDevice for Htif<D> {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
        self.inner.read_u8(addr)
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
        self.inner.read_u16(addr)
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        self.inner.read_u32(addr)
    }
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError> {
        self.inner.write_u8(addr, val)
    }
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError> {
        self.inner.write_u16(addr, val)
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        self.inner.write_u32(addr, val)?;
        if addr == self.tohost {
            self.low = val;
        } else if addr == self.tohost.wrapping_add(4) {
            self.command(val);
        }
        Ok(())
    }
    fn tick(&mut self, cycles: u64) {
        self.inner.tick(cycles);
    }
    fn irq_lines(&self) -> u32 {
        self.inner.irq_lines()
    }
    fn exit_request(&self) -> Option<u32> {
        self.exit.or_else(|| self.inner.exit_request())
    }
    fn is_mapped(&self, addr: u32) -> bool {
        self.inner.is_mapped(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ram;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn htif() -> Htif<Ram> {
        Htif::new(Ram::new(64), 0x10)
    }

    #[test]
    fn test_exit_codes() {
        let mut h = htif();
        h.write_u32(0x10, 1).unwrap();
        assert_eq!(h.exit_request(), None, "needs the upper word");
        h.write_u32(0x14, 0).unwrap();
        assert_eq!(h.exit_request(), Some(0));
        assert_eq!(h.read_u32(0x10), Ok(1), "memory is written through");

        let mut h = htif();
        h.write_u32(0x10, (5 << 1) | 1).unwrap();
        h.write_u32(0x14, 0).unwrap();
        assert_eq!(h.exit_request(), Some(5));
        // Sticky: a later pass does not override the failure.
        h.write_u32(0x10, 1).unwrap();
        h.write_u32(0x14, 0).unwrap();
        assert_eq!(h.exit_request(), Some(5));
    }

    #[test]
    fn test_console_and_ignored_commands() {
        let out = Rc::new(RefCell::new(Vec::new()));
        let mut h = htif();
        let sink = out.clone();
        h.set_sink(Some(Box::new(move |b| sink.borrow_mut().push(b))));
        for &b in b"ok" {
            h.write_u32(0x10, b as u32).unwrap();
            h.write_u32(0x14, 0x0101_0000).unwrap();
        }
        assert_eq!(&*out.borrow(), b"ok");
        assert_eq!(h.exit_request(), None, "odd 'k' is not an exit");

        // Syscall (even payload) and unknown devices are ignored.
        h.write_u32(0x10, 0x100).unwrap();
        h.write_u32(0x14, 0).unwrap();
        h.write_u32(0x10, 1).unwrap();
        h.write_u32(0x14, 0x0200_0000).unwrap();
        assert_eq!(h.exit_request(), None);
        // Byte stores and other addresses only write memory.
        h.write_u8(0x14, 0).unwrap();
        h.write_u32(0x18, 1).unwrap();
        assert_eq!(h.exit_request(), None);
    }
}
