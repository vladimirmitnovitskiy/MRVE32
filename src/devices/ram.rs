use crate::bus::{BusDevice, BusError};
use crate::types::Reg;

pub struct Ram {
    data: Vec<u8>,
}

impl Ram {
    pub fn new(size: usize) -> Self{
        Self {
            data: vec![0; size],
        }
    }
}

impl BusDevice for Ram {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
        let index = addr as usize;
        if index < self.data.len(){
            Ok(self.data[index])
        }
        else {
            Err(BusError::AddressNotFound(addr))
        }
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
        let index = addr as usize;
        if index + 2 <= self.data.len(){
            let bytes: [u8; 2] = self.data[index..index + 2].try_into().unwrap();
            Ok(u16::from_le_bytes(bytes))
        }
        else {
            Err(BusError::AddressNotFound(addr))
        }
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        let index = addr as usize;
        if index + 4 <= self.data.len(){
            let bytes: [u8; 4] = self.data[index..index + 4].try_into().unwrap();
            Ok(u32::from_le_bytes(bytes))
        }
        else {
            Err(BusError::AddressNotFound(addr))
        }
    }
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError> {
        let index = addr as usize;
        if index < self.data.len() {
            self.data[index] = val;
            Ok(())
        }
        else {
            Err(BusError::AddressNotFound(addr))
        }
    }
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError> {
        let index = addr as usize;
        if index + 2 <= self.data.len(){
            let bytes = val.to_le_bytes();
            self.data[index..index + 2].copy_from_slice(&bytes);
            Ok(())
        }
        else {
            Err(BusError::AddressNotFound(addr))
        }
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        let index = addr as usize;
        if index + 4 <= self.data.len(){
            let bytes = val.to_le_bytes();
            self.data[index..index + 4].copy_from_slice(&bytes);
            Ok(())
        }
        else {
            Err(BusError::AddressNotFound(addr))
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const RAM_SIZE: usize = 64;

    fn setup_ram() -> Ram {
        Ram::new(RAM_SIZE)
    }

    #[test]
    fn test_initialization() {
        let mut ram = setup_ram();
        for addr in 0..RAM_SIZE as Reg {
            assert_eq!(ram.read_u8(addr).unwrap(), 0);
        }
    }

    #[test]
    fn test_u8_read_write() {
        let mut ram = setup_ram();
        
        assert!(ram.write_u8(0, 0x42).is_ok());
        assert_eq!(ram.read_u8(0).unwrap(), 0x42);

        // Запись и чтение по последнему доступному адресу
        let last_addr = (RAM_SIZE - 1) as Reg;
        assert!(ram.write_u8(last_addr, 0xFF).is_ok());
        assert_eq!(ram.read_u8(last_addr).unwrap(), 0xFF);
    }

    #[test]
    fn test_u16_read_write_and_endianness() {
        let mut ram = setup_ram();
        let val: u16 = 0x1234;

        assert!(ram.write_u16(4, val).is_ok());
        assert_eq!(ram.read_u16(4).unwrap(), val);

        // Проверка Little-Endian (младший байт по младшему адресу)
        assert_eq!(ram.read_u8(4).unwrap(), 0x34);
        assert_eq!(ram.read_u8(5).unwrap(), 0x12);

        // Запись по последнему доступному для u16 адресу
        let last_valid_addr = (RAM_SIZE - 2) as Reg;
        assert!(ram.write_u16(last_valid_addr, 0xAABB).is_ok());
        assert_eq!(ram.read_u16(last_valid_addr).unwrap(), 0xAABB);
    }

    #[test]
    fn test_u32_read_write_and_endianness() {
        let mut ram = setup_ram();
        let val: u32 = 0x12345678;

        assert!(ram.write_u32(8, val).is_ok());
        assert_eq!(ram.read_u32(8).unwrap(), val);

        // Проверка Little-Endian
        assert_eq!(ram.read_u8(8).unwrap(), 0x78);
        assert_eq!(ram.read_u8(9).unwrap(), 0x56);
        assert_eq!(ram.read_u8(10).unwrap(), 0x34);
        assert_eq!(ram.read_u8(11).unwrap(), 0x12);

        // Запись по последнему доступному для u32 адресу
        let last_valid_addr = (RAM_SIZE - 4) as Reg;
        assert!(ram.write_u32(last_valid_addr, 0xDEADBEEF).is_ok());
        assert_eq!(ram.read_u32(last_valid_addr).unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn test_out_of_bounds_errors() {
        let mut ram = setup_ram();
        let ram_len = RAM_SIZE as Reg;

        // u8 out of bounds
        assert!(matches!(
            ram.read_u8(ram_len),
            Err(BusError::AddressNotFound(addr)) if addr == ram_len
        ));
        assert!(matches!(
            ram.write_u8(ram_len, 0x11),
            Err(BusError::AddressNotFound(addr)) if addr == ram_len
        ));

        // u16 out of bounds (RAM_SIZE - 1 захватывает 1 байт за пределами памяти)
        let oob_u16_addr = (RAM_SIZE - 1) as Reg;
        assert!(matches!(
            ram.read_u16(oob_u16_addr),
            Err(BusError::AddressNotFound(addr)) if addr == oob_u16_addr
        ));
        assert!(matches!(
            ram.write_u16(oob_u16_addr, 0x1234),
            Err(BusError::AddressNotFound(addr)) if addr == oob_u16_addr
        ));

        // u32 out of bounds (RAM_SIZE - 3 захватывает байты за пределами памяти)
        let oob_u32_addr = (RAM_SIZE - 3) as Reg;
        assert!(matches!(
            ram.read_u32(oob_u32_addr),
            Err(BusError::AddressNotFound(addr)) if addr == oob_u32_addr
        ));
        assert!(matches!(
            ram.write_u32(oob_u32_addr, 0x12345678),
            Err(BusError::AddressNotFound(addr)) if addr == oob_u32_addr
        ));
    }

    #[test]
    fn test_unaligned_access() {
        let mut ram = setup_ram();

        // Проверка корректности работы при невыровненных адресах
        assert!(ram.write_u16(1, 0xCAFE).is_ok());
        assert_eq!(ram.read_u16(1).unwrap(), 0xCAFE);

        assert!(ram.write_u32(3, 0x01020304).is_ok());
        assert_eq!(ram.read_u32(3).unwrap(), 0x01020304);
    }
}