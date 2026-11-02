//! Core-Local Interruptor (CLINT) for a single hart, compatible with the
//! SiFive/QEMU `virt` memory map:
//!
//! | offset   | register            |
//! |----------|---------------------|
//! | `0x0000` | `msip` (bit 0)      |
//! | `0x4000` | `mtimecmp` (64-bit) |
//! | `0xBFF8` | `mtime` (64-bit)    |
//!
//! `mtime` advances by one every `divider` CPU cycles. The device asserts
//! `MTIP` while `mtime >= mtimecmp` and `MSIP` while `msip[0]` is set.

use crate::bus::{BusDevice, BusError};
use crate::cpu::csr::{MIP_MSIP, MIP_MTIP};
use crate::types::Reg;

/// Size of the CLINT address window, for [`crate::Bus::map_device`].
pub const CLINT_SIZE: Reg = 0x1_0000;

const MSIP: Reg = 0x0000;
const MTIMECMP: Reg = 0x4000;
const MTIME: Reg = 0xBFF8;

#[derive(Debug, Clone)]
pub struct Clint {
    msip: bool,
    mtimecmp: u64,
    mtime: u64,
    divider: u64,
    subcycles: u64,
}

impl Default for Clint {
    fn default() -> Self {
        Self::new()
    }
}

impl Clint {
    /// A CLINT whose `mtime` increments once per CPU cycle.
    pub fn new() -> Self {
        Self::with_divider(1)
    }

    /// A CLINT whose `mtime` increments once every `divider` CPU cycles
    /// (e.g. 10 for a 100 MHz core with a 10 MHz timebase).
    ///
    /// # Panics
    /// If `divider` is zero.
    pub fn with_divider(divider: u64) -> Self {
        assert!(divider > 0, "CLINT divider must be non-zero");
        Self {
            msip: false,
            // Reset value is unspecified by the spec; all-ones keeps the timer
            // interrupt deasserted until software programs mtimecmp.
            mtimecmp: u64::MAX,
            mtime: 0,
            divider,
            subcycles: 0,
        }
    }

    pub fn mtime(&self) -> u64 {
        self.mtime
    }

    pub fn set_mtime(&mut self, mtime: u64) {
        self.mtime = mtime;
    }

    pub fn mtimecmp(&self) -> u64 {
        self.mtimecmp
    }

    fn read_word(&self, offset: Reg) -> Option<u32> {
        Some(match offset {
            MSIP => self.msip as u32,
            o if o == MTIMECMP => self.mtimecmp as u32,
            o if o == MTIMECMP + 4 => (self.mtimecmp >> 32) as u32,
            o if o == MTIME => self.mtime as u32,
            o if o == MTIME + 4 => (self.mtime >> 32) as u32,
            _ => return None,
        })
    }

    fn write_word(&mut self, offset: Reg, val: u32) -> Option<()> {
        let lo = |r: u64| (r & !0xFFFF_FFFF) | val as u64;
        let hi = |r: u64| (r & 0xFFFF_FFFF) | ((val as u64) << 32);
        match offset {
            MSIP => self.msip = val & 1 != 0,
            o if o == MTIMECMP => self.mtimecmp = lo(self.mtimecmp),
            o if o == MTIMECMP + 4 => self.mtimecmp = hi(self.mtimecmp),
            o if o == MTIME => self.mtime = lo(self.mtime),
            o if o == MTIME + 4 => self.mtime = hi(self.mtime),
            _ => return None,
        }
        Some(())
    }

    /// Reads `size` bytes (1, 2 or 4) at a naturally aligned address.
    fn read_sized(&self, addr: Reg, size: Reg) -> Result<u32, BusError> {
        if !addr.is_multiple_of(size) {
            return Err(BusError::InvalidAccess(addr));
        }
        let word = self
            .read_word(addr & !0b11)
            .ok_or(BusError::AddressNotFound(addr))?;
        let shift = (addr & 0b11) * 8;
        let mask = if size == 4 {
            u32::MAX
        } else {
            (1 << (size * 8)) - 1
        };
        Ok((word >> shift) & mask)
    }

    /// Writes `size` bytes (1, 2 or 4) at a naturally aligned address.
    fn write_sized(&mut self, addr: Reg, size: Reg, val: u32) -> Result<(), BusError> {
        if !addr.is_multiple_of(size) {
            return Err(BusError::InvalidAccess(addr));
        }
        let base = addr & !0b11;
        let word = self
            .read_word(base)
            .ok_or(BusError::AddressNotFound(addr))?;
        let shift = (addr & 0b11) * 8;
        let mask = if size == 4 {
            u32::MAX
        } else {
            ((1 << (size * 8)) - 1) << shift
        };
        let new = (word & !mask) | ((val << shift) & mask);
        self.write_word(base, new)
            .ok_or(BusError::AddressNotFound(addr))
    }
}

impl BusDevice for Clint {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
        self.read_sized(addr, 1).map(|v| v as u8)
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
        self.read_sized(addr, 2).map(|v| v as u16)
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        self.read_sized(addr, 4)
    }
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError> {
        self.write_sized(addr, 1, val as u32)
    }
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError> {
        self.write_sized(addr, 2, val as u32)
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        self.write_sized(addr, 4, val)
    }

    fn tick(&mut self, cycles: u64) {
        let total = self.subcycles + cycles;
        self.mtime = self.mtime.wrapping_add(total / self.divider);
        self.subcycles = total % self.divider;
    }

    fn irq_lines(&self) -> Reg {
        let mut lines = 0;
        if self.msip {
            lines |= MIP_MSIP;
        }
        if self.mtime >= self.mtimecmp {
            lines |= MIP_MTIP;
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reset_state_has_no_interrupts() {
        let clint = Clint::new();
        assert_eq!(clint.irq_lines(), 0);
        assert_eq!(clint.mtime(), 0);
        assert_eq!(clint.mtimecmp(), u64::MAX);
    }

    #[test]
    fn test_msip_drives_software_interrupt() {
        let mut clint = Clint::new();
        clint.write_u32(MSIP, 0xFFFF_FFFF).unwrap();
        assert_eq!(clint.read_u32(MSIP), Ok(1)); // only bit 0 is implemented
        assert_eq!(clint.irq_lines(), MIP_MSIP);
        clint.write_u32(MSIP, 0).unwrap();
        assert_eq!(clint.irq_lines(), 0);
    }

    #[test]
    fn test_timer_interrupt_when_mtime_reaches_mtimecmp() {
        let mut clint = Clint::new();
        clint.write_u32(MTIMECMP, 10).unwrap();
        clint.write_u32(MTIMECMP + 4, 0).unwrap();
        assert_eq!(clint.mtimecmp(), 10);

        clint.tick(9);
        assert_eq!(clint.irq_lines(), 0);
        clint.tick(1);
        assert_eq!(clint.irq_lines(), MIP_MTIP);

        // Moving mtimecmp forward clears the pending timer interrupt
        clint.write_u32(MTIMECMP, 20).unwrap();
        assert_eq!(clint.irq_lines(), 0);
    }

    #[test]
    fn test_mtime_64bit_access_and_divider() {
        let mut clint = Clint::with_divider(4);
        clint.write_u32(MTIME, 0xFFFF_FFFF).unwrap();
        clint.write_u32(MTIME + 4, 0x1).unwrap();
        assert_eq!(clint.mtime(), 0x1_FFFF_FFFF);

        clint.tick(3);
        assert_eq!(clint.mtime(), 0x1_FFFF_FFFF);
        clint.tick(1);
        assert_eq!(clint.mtime(), 0x2_0000_0000);
        assert_eq!(clint.read_u32(MTIME), Ok(0));
        assert_eq!(clint.read_u32(MTIME + 4), Ok(2));

        clint.tick(10); // 14 subcycles -> +2, remainder 2... carried over
        assert_eq!(clint.mtime(), 0x2_0000_0002);
        clint.tick(2);
        assert_eq!(clint.mtime(), 0x2_0000_0003);
    }

    #[test]
    fn test_byte_and_halfword_access() {
        let mut clint = Clint::new();
        clint.write_u32(MTIMECMP, 0x1122_3344).unwrap();
        assert_eq!(clint.read_u8(MTIMECMP + 1), Ok(0x33));
        assert_eq!(clint.read_u16(MTIMECMP + 2), Ok(0x1122));

        clint.write_u8(MTIMECMP + 3, 0xAA).unwrap();
        clint.write_u16(MTIMECMP, 0xBEEF).unwrap();
        assert_eq!(clint.read_u32(MTIMECMP), Ok(0xAA22_BEEF));
    }

    #[test]
    fn test_invalid_accesses() {
        let mut clint = Clint::new();
        assert_eq!(clint.read_u32(0x4002), Err(BusError::InvalidAccess(0x4002)));
        assert_eq!(
            clint.write_u16(0x0001, 1),
            Err(BusError::InvalidAccess(0x0001))
        );
        assert_eq!(
            clint.read_u32(0x0100),
            Err(BusError::AddressNotFound(0x0100))
        );
    }

    #[test]
    #[should_panic]
    fn test_zero_divider_panics() {
        let _ = Clint::with_divider(0);
    }
}
