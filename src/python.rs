use crate::devices::clint::{CLINT_SIZE, Clint};
use crate::{Bus, BusDevice, Cpu, Ram, Reg, TrapCause};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

/// Информация о взятом trap (исключение или прерывание уже обработано CPU)
#[pyclass(name = "Trap", frozen, get_all)]
pub struct PyTrap {
    /// Значение mcause (бит 31 установлен для прерываний)
    cause: Reg,
    /// True для прерывания, False для исключения
    is_interrupt: bool,
    /// Значение mtval
    tval: Reg,
    /// Значение mepc
    epc: Reg,
    /// Текстовое описание
    description: String,
}

#[pymethods]
impl PyTrap {
    fn __repr__(&self) -> String {
        format!(
            "Trap(cause=0x{:08X}, is_interrupt={}, tval=0x{:08X}, epc=0x{:08X}, description='{}')",
            self.cause,
            if self.is_interrupt { "True" } else { "False" },
            self.tval,
            self.epc,
            self.description
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
    /// Конструктор: создает CPU, подключает RAM заданного размера по адресу pc
    /// и CLINT (таймер + программное прерывание) по адресу clint_base
    /// (None — без CLINT)
    #[new]
    #[pyo3(signature = (pc=0x1000, ram_size=4096, clint_base=Some(0x0200_0000)))]
    pub fn new(pc: Reg, ram_size: usize, clint_base: Option<Reg>) -> PyResult<Self> {
        let ram_len = Reg::try_from(ram_size)
            .map_err(|_| PyValueError::new_err("ram_size must fit in 32 bits"))?;
        let mut bus = Bus::new();
        bus.map_device(pc, ram_len, Box::new(Ram::new(ram_size)))
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        if let Some(base) = clint_base {
            bus.map_device(base, CLINT_SIZE, Box::new(Clint::new()))
                .map_err(|e| PyValueError::new_err(e.to_string()))?;
        }
        let cpu = Cpu::new(pc);
        Ok(Self { cpu, bus })
    }

    /// Сделать один такт: принять прерывание либо выполнить инструкцию.
    /// Возвращает None, либо Trap, если был взят trap
    /// (pc уже указывает на обработчик из mtvec).
    pub fn step(&mut self) -> Option<PyTrap> {
        self.cpu.step(&mut self.bus).map(|trap| PyTrap {
            cause: trap.cause.mcause(),
            is_interrupt: matches!(trap.cause, TrapCause::Interrupt(_)),
            tval: trap.cause.tval(),
            epc: trap.epc,
            description: trap.cause.to_string(),
        })
    }

    /// True, если CPU ожидает прерывания в WFI
    #[getter]
    pub fn is_waiting(&self) -> bool {
        self.cpu.is_waiting()
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
