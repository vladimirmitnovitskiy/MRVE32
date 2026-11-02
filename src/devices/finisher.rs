//! Test finisher (`sifive,test0`, as in QEMU's `virt` machine): firmware
//! stops the emulator with an exit status.
//!
//! A 32-bit write to offset 0:
//!
//! | value | effect |
//! |---|---|
//! | `0x5555` | pass: exit code 0 |
//! | `(code << 16) \| 0x3333` | fail: exit code `code` (QEMU uses the same encoding) |
//!
//! Any other value (including `0x7777`, reset, which is not supported) is a
//! store access fault. Once written, the request is sticky: the machine
//! stops after every step. Reads return 0.

use crate::bus::{BusDevice, BusError};
use crate::types::Reg;

/// Address window of the finisher.
pub const FINISHER_SIZE: Reg = 0x1000;

pub const FINISHER_PASS: u32 = 0x5555;
pub const FINISHER_FAIL: u32 = 0x3333;

#[derive(Debug, Default)]
pub struct TestFinisher {
    exit: Option<u32>,
}

impl TestFinisher {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BusDevice for TestFinisher {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
        self.read_u32(addr).map(|v| v as u8)
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
        self.read_u32(addr).map(|v| v as u16)
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        if addr < FINISHER_SIZE {
            Ok(0)
        } else {
            Err(BusError::AddressNotFound(addr))
        }
    }
    fn write_u8(&mut self, addr: Reg, _: u8) -> Result<(), BusError> {
        Err(BusError::InvalidAccess(addr))
    }
    fn write_u16(&mut self, addr: Reg, _: u16) -> Result<(), BusError> {
        Err(BusError::InvalidAccess(addr))
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        if addr != 0 {
            return Err(BusError::InvalidAccess(addr));
        }
        self.exit = Some(match val & 0xFFFF {
            FINISHER_PASS => 0,
            FINISHER_FAIL => val >> 16,
            _ => return Err(BusError::InvalidAccess(addr)),
        });
        Ok(())
    }
    fn exit_request(&self) -> Option<u32> {
        self.exit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pass_and_fail() {
        let mut f = TestFinisher::new();
        assert_eq!(f.exit_request(), None);
        f.write_u32(0, FINISHER_PASS).unwrap();
        assert_eq!(f.exit_request(), Some(0));

        let mut f = TestFinisher::new();
        f.write_u32(0, 42 << 16 | FINISHER_FAIL).unwrap();
        assert_eq!(f.exit_request(), Some(42));
    }

    #[test]
    fn test_invalid_writes_fault() {
        let mut f = TestFinisher::new();
        assert_eq!(f.write_u32(0, 0x7777), Err(BusError::InvalidAccess(0)));
        assert_eq!(
            f.write_u32(4, FINISHER_PASS),
            Err(BusError::InvalidAccess(4))
        );
        assert_eq!(f.write_u8(0, 0x55), Err(BusError::InvalidAccess(0)));
        assert_eq!(f.exit_request(), None);
        assert_eq!(f.read_u32(0), Ok(0));
    }
}
