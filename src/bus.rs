use crate::types::Reg;

struct DeviceEntry {
    start: Reg,
    length: Reg,
    device: Box<dyn BusDevice>,
}

pub struct Bus {
    devices: Vec<DeviceEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusError {
    AddressNotFound(Reg),

}


pub trait BusDevice{
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError>;
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError>;
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError>;
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError>;
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError>;
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError>;
}

impl Bus {
    pub fn new() -> Self {
        Self { 
            devices: Vec::new()
        }
    }
    pub fn map_device(&mut self, start: Reg, length: Reg, device: Box<dyn BusDevice>){
        self.devices.push(DeviceEntry {
                start,
                length,
                device,
            });
    }

    fn find_device(&mut self, addr: Reg) -> Result<(&mut Box<dyn BusDevice>, Reg), BusError>{
        for entry in &mut self.devices {
            if addr >= entry.start {
                let offset = addr - entry.start;
                if offset < entry.length {
                    return Ok((&mut entry.device, offset));
                }
            }
        }   
        Err(BusError::AddressNotFound(addr))
            
    }
}

impl BusDevice for Bus {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
        let (device, offset) = self.find_device(addr)?;
        device.read_u8(offset)
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
        let (device, offset) = self.find_device(addr)?;
        device.read_u16(offset)
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        let (device, offset) = self.find_device(addr)?;
        device.read_u32(offset)
    }
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError> {
        let (device, offset) = self.find_device(addr)?;
        device.write_u8(offset, val)
    }
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError> {
        let (device, offset) = self.find_device(addr)?;
        device.write_u16(offset, val)
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        let (device, offset) = self.find_device(addr)?;
        device.write_u32(offset, val)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // Простая реализация BusDevice для тестирования (RAM/буфер фиксированного размера)
    struct MockMemory {
        data: Vec<u8>,
    }

    impl MockMemory {
        fn new(size: usize) -> Self {
            Self {
                data: vec![0; size],
            }
        }
    }

    impl BusDevice for MockMemory {
        fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
            let idx = addr as usize;
            if idx < self.data.len() {
                Ok(self.data[idx])
            } else {
                Err(BusError::AddressNotFound(addr))
            }
        }

        fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
            let idx = addr as usize;
            if idx + 1 < self.data.len() {
                let bytes = [self.data[idx], self.data[idx + 1]];
                Ok(u16::from_le_bytes(bytes))
            } else {
                Err(BusError::AddressNotFound(addr))
            }
        }

        fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
            let idx = addr as usize;
            if idx + 3 < self.data.len() {
                let bytes = [
                    self.data[idx],
                    self.data[idx + 1],
                    self.data[idx + 2],
                    self.data[idx + 3],
                ];
                Ok(u32::from_le_bytes(bytes))
            } else {
                Err(BusError::AddressNotFound(addr))
            }
        }

        fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError> {
            let idx = addr as usize;
            if idx < self.data.len() {
                self.data[idx] = val;
                Ok(())
            } else {
                Err(BusError::AddressNotFound(addr))
            }
        }

        fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError> {
            let idx = addr as usize;
            if idx + 1 < self.data.len() {
                let bytes = val.to_le_bytes();
                self.data[idx..idx + 2].copy_from_slice(&bytes);
                Ok(())
            } else {
                Err(BusError::AddressNotFound(addr))
            }
        }

        fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
            let idx = addr as usize;
            if idx + 3 < self.data.len() {
                let bytes = val.to_le_bytes();
                self.data[idx..idx + 4].copy_from_slice(&bytes);
                Ok(())
            } else {
                Err(BusError::AddressNotFound(addr))
            }
        }
    }

    #[test]
    fn test_read_write_with_offset() {
        let mut bus = Bus::new();
        // Мапим 256 байт памяти по адресу 0x1000
        bus.map_device(0x1000, 0x100, Box::new(MockMemory::new(0x100)));

        // Запись u8, u16, u32
        bus.write_u8(0x1000, 0xAA).unwrap();
        bus.write_u16(0x1004, 0x1234).unwrap();
        bus.write_u32(0x1010, 0xDEADBEEF).unwrap();

        // Проверка чтения
        assert_eq!(bus.read_u8(0x1000), Ok(0xAA));
        assert_eq!(bus.read_u16(0x1004), Ok(0x1234));
        assert_eq!(bus.read_u32(0x1010), Ok(0xDEADBEEF));
    }

    #[test]
    fn test_multiple_mapped_devices() {
        let mut bus = Bus::new();
        let dev1 = Box::new(MockMemory::new(0x10));
        let dev2 = Box::new(MockMemory::new(0x10));

        bus.map_device(0x0000, 0x10, dev1);
        bus.map_device(0x0100, 0x10, dev2);

        bus.write_u8(0x0002, 0x11).unwrap();
        bus.write_u8(0x0102, 0x22).unwrap();

        // Проверяем изоляцию адресов между разными устройствами
        assert_eq!(bus.read_u8(0x0002), Ok(0x11));
        assert_eq!(bus.read_u8(0x0102), Ok(0x22));
    }

    #[test]
    fn test_address_not_found() {
        let mut bus = Bus::new();
        bus.map_device(0x2000, 0x100, Box::new(MockMemory::new(0x100)));

        // Адрес до диапазона
        assert_eq!(bus.read_u8(0x1FFF), Err(BusError::AddressNotFound(0x1FFF)));
        // Адрес ровно на верхней границе (длина 0x100 -> допустимо 0x2000..=0x20FF)
        assert_eq!(bus.read_u8(0x2100), Err(BusError::AddressNotFound(0x2100)));
        // Неизвестный адрес при записи
        assert_eq!(
            bus.write_u32(0x5000, 0x1),
            Err(BusError::AddressNotFound(0x5000))
        );
    }

    #[test]
    fn test_boundary_conditions() {
        let mut bus = Bus::new();
        // Диапазон [0x100, 0x104) -> длина 4 байта
        bus.map_device(0x100, 4, Box::new(MockMemory::new(4)));

        // Самый первый байт
        assert!(bus.write_u8(0x100, 0x01).is_ok());
        // Самый последний валидный байт (0x100 + 4 - 1 = 0x103)
        assert!(bus.write_u8(0x103, 0x02).is_ok());
        assert_eq!(bus.read_u8(0x103), Ok(0x02));

        // Сразу за границей
        assert_eq!(bus.read_u8(0x104), Err(BusError::AddressNotFound(0x104)));
    }

    #[test]
    fn test_nested_bus() {
        // Поскольку Bus реализует BusDevice, шины могут быть вложенными
        let mut root_bus = Bus::new();
        let mut sub_bus = Bus::new();

        sub_bus.map_device(0x00, 0x20, Box::new(MockMemory::new(0x20)));
        root_bus.map_device(0x1000, 0x100, Box::new(sub_bus));

        // Доступ через корневую шину ко вложенной
        root_bus.write_u16(0x1008, 0xCAFE).unwrap();
        assert_eq!(root_bus.read_u16(0x1008), Ok(0xCAFE));
    }
}