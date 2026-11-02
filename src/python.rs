use crate::{Bus, BusDevice, Cpu, Ram, Reg};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

/// Информация о взятом trap (исключение уже обработано CPU)
#[pyclass(name = "Trap", frozen, get_all)]
pub struct PyTrap {
    /// Значение mcause
    cause: Reg,
    /// Значение mtval
    tval: Reg,
    /// Адрес инструкции, вызвавшей исключение
    epc: Reg,
    /// Текстовое описание
    description: String,
}

#[pymethods]
impl PyTrap {
    fn __repr__(&self) -> String {
        format!(
            "Trap(cause={}, tval=0x{:08X}, epc=0x{:08X}, description='{}')",
            self.cause, self.tval, self.epc, self.description
        )
    }
}

#[pyclass(unsendable)]
pub struct Emulator {
    cpu: Cpu,
    bus: Bus,
}

#[pymethods]
impl Emulator {
    /// Конструктор: создает CPU и подключает RAM заданного размера по адресу pc
    #[new]
    #[pyo3(signature = (pc=0x1000, ram_size=4096))]
    pub fn new(pc: Reg, ram_size: usize) -> Self {
        let mut bus = Bus::new();
        bus.map_device(pc, ram_size as Reg, Box::new(Ram::new(ram_size)))
            .expect("Failed  to map RAM");
        let cpu = Cpu::new(pc);
        Self { cpu, bus }
    }

    /// Сделать один такт (Fetch -> Decode -> Execute).
    /// Возвращает None, либо Trap, если инструкция вызвала исключение
    /// (trap уже взят: pc указывает на mtvec).
    pub fn step(&mut self) -> Option<PyTrap> {
        self.cpu.step(&mut self.bus).map(|trap| PyTrap {
            cause: trap.exception.code(),
            tval: trap.exception.tval(),
            epc: trap.epc,
            description: trap.exception.to_string(),
        })
    }

    /// Чтение регистра x0..x31
    pub fn read_reg(&self, reg: usize) -> Reg {
        self.cpu.read_reg(reg)
    }

    /// Запись в регистр
    pub fn write_reg(&mut self, reg: usize, val: Reg) {
        self.cpu.write_reg(reg, val);
    }

    /// Геттер для PC (в Python доступен как emu.pc)
    #[getter]
    pub fn pc(&self) -> Reg {
        self.cpu.pc
    }

    /// Сеттер для PC
    #[setter]
    pub fn set_pc(&mut self, pc: Reg) {
        self.cpu.pc = pc;
    }

    /// Загрузить массив 32-битных опкодов в память
    pub fn load_program(&mut self, start_addr: Reg, program: Vec<u32>) -> PyResult<()> {
        for (i, &word) in program.iter().enumerate() {
            let addr = start_addr.wrapping_add((i * 4) as Reg);
            self.bus
                .write_u32(addr, word)
                .map_err(|e| PyRuntimeError::new_err(format!("{:?}", e)))?;
        }
        Ok(())
    }

    // CSR
    pub fn read_csr(&self, csr: u16) -> PyResult<Reg> {
        self.cpu
            .read_csr(csr)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn write_csr(&mut self, csr: u16, val: Reg) -> PyResult<()> {
        self.cpu
            .write_csr(csr, val)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    // Чтение и запись памяти напрямую через шину
    pub fn read_u8(&mut self, addr: Reg) -> PyResult<u8> {
        self.bus
            .read_u8(addr)
            .map_err(|e| PyRuntimeError::new_err(format!("{}", e)))
    }

    pub fn read_u32(&mut self, addr: Reg) -> PyResult<u32> {
        self.bus
            .read_u32(addr)
            .map_err(|e| PyRuntimeError::new_err(format!("{}", e)))
    }

    pub fn write_u8(&mut self, addr: Reg, val: u8) -> PyResult<()> {
        self.bus
            .write_u8(addr, val)
            .map_err(|e| PyRuntimeError::new_err(format!("{}", e)))
    }

    pub fn write_u32(&mut self, addr: Reg, val: u32) -> PyResult<()> {
        self.bus
            .write_u32(addr, val)
            .map_err(|e| PyRuntimeError::new_err(format!("{}", e)))
    }
}

/// Точка входа модуля Python: import mrve32
#[pymodule]
fn mrve32(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Emulator>()?;
    m.add_class::<PyTrap>()?;
    Ok(())
}
