//! Python bindings (`mrve32._native`), built with the `python` feature.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyIndexError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::PyBytes;

use crate::devices::clint::{CLINT_SIZE, Clint};
use crate::plugin::RawDevice;
use crate::plugin::python::{into_capsule, take_from_capsule};
use crate::{Bus, BusDevice, Cpu, ElfImage, Machine, Ram, Reg, StopReason, Trap, TrapCause};

create_exception!(
    mrve32,
    BusError,
    PyException,
    "A bus access failed (unmapped address, invalid width/alignment, device busy)."
);
create_exception!(
    mrve32,
    ElfError,
    PyException,
    "An ELF file could not be parsed or loaded."
);

fn bus_err(e: crate::BusError) -> PyErr {
    BusError::new_err(e.to_string())
}

/// A trap taken by the CPU (exception or interrupt); already handled, `pc`
/// points to the trap vector.
#[pyclass(name = "Trap", frozen, get_all, from_py_object)]
#[derive(Clone)]
pub struct PyTrap {
    /// `mcause` value (bit 31 set for interrupts).
    cause: Reg,
    is_interrupt: bool,
    /// `mtval` value.
    tval: Reg,
    /// `mepc` value.
    epc: Reg,
    description: String,
}

impl From<Trap> for PyTrap {
    fn from(trap: Trap) -> Self {
        Self {
            cause: trap.cause.mcause(),
            is_interrupt: matches!(trap.cause, TrapCause::Interrupt(_)),
            tval: trap.cause.tval(),
            epc: trap.epc,
            description: trap.cause.to_string(),
        }
    }
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

/// Result of `Emulator.run`.
#[pyclass(name = "RunResult", frozen, get_all)]
pub struct PyRunResult {
    /// Steps (clock cycles) executed by the call.
    steps: u64,
    /// "max_steps", "breakpoint", "trap" or "idle".
    reason: &'static str,
    /// The trap that stopped the run (reason == "trap"), else None.
    trap: Option<PyTrap>,
}

#[pymethods]
impl PyRunResult {
    fn __repr__(&self) -> String {
        match &self.trap {
            Some(trap) => format!(
                "RunResult(steps={}, reason='{}', trap={})",
                self.steps,
                self.reason,
                trap.__repr__()
            ),
            None => format!("RunResult(steps={}, reason='{}')", self.steps, self.reason),
        }
    }
}

// --- Devices -----------------------------------------------------------------

/// Main memory, attachable with `Emulator.map_device`.
#[pyclass(name = "Ram", unsendable)]
pub struct PyRam {
    device: Option<Ram>,
    size: Reg,
}

#[pymethods]
impl PyRam {
    #[new]
    fn new(size: usize) -> PyResult<Self> {
        let len = window_size(size)?;
        Ok(Self {
            device: Some(Ram::new(size)),
            size: len,
        })
    }

    #[getter]
    fn size(&self) -> Reg {
        self.size
    }

    fn __mrve32_device__<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.take().ok_or_else(already_attached)?;
        into_capsule(py, RawDevice::new(device, self.size))
    }
}

/// Core-local interruptor (`msip`, `mtime`, `mtimecmp`; SiFive memory map).
#[pyclass(name = "Clint", unsendable)]
pub struct PyClint {
    device: Option<Clint>,
}

#[pymethods]
impl PyClint {
    /// `mtime` advances by one every `divider` CPU cycles.
    #[new]
    #[pyo3(signature = (divider = 1))]
    fn new(divider: u64) -> PyResult<Self> {
        if divider == 0 {
            return Err(PyValueError::new_err("divider must be >= 1"));
        }
        Ok(Self {
            device: Some(Clint::with_divider(divider)),
        })
    }

    #[getter]
    fn size(&self) -> Reg {
        CLINT_SIZE
    }

    fn __mrve32_device__<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.take().ok_or_else(already_attached)?;
        into_capsule(py, RawDevice::new(device, CLINT_SIZE))
    }
}

fn already_attached() -> PyErr {
    PyRuntimeError::new_err("the device is already attached to an emulator")
}

fn window_size(size: usize) -> PyResult<Reg> {
    Reg::try_from(size)
        .ok()
        .filter(|&len| len > 0)
        .ok_or_else(|| PyValueError::new_err("size must be in 1..2**32"))
}

/// First error raised by a Python device callback, re-raised by the
/// `Emulator` method that triggered the access.
type ErrorSlot = Rc<RefCell<Option<PyErr>>>;

/// A device implemented in Python (duck-typed, see `Emulator.map_device`).
struct PyDevice {
    obj: Py<PyAny>,
    has_tick: bool,
    has_irq_lines: bool,
    error: ErrorSlot,
}

impl PyDevice {
    /// A `mrve32.BusError` from the device is an ordinary access fault; any
    /// other exception is recorded and aborts the current emulator call.
    fn fail(&self, py: Python<'_>, err: PyErr, addr: Reg) -> crate::BusError {
        if !err.is_instance_of::<BusError>(py) {
            self.error.borrow_mut().get_or_insert(err);
        }
        crate::BusError::InvalidAccess(addr)
    }

    fn read(&mut self, addr: Reg, width: u32) -> Result<u32, crate::BusError> {
        Python::attach(|py| {
            let value = self
                .obj
                .call_method1(py, "read", (addr, width))
                .and_then(|v| v.extract::<u32>(py))
                .and_then(|v| {
                    if width < 4 && v >> (8 * width) != 0 {
                        Err(PyValueError::new_err(format!(
                            "read({addr:#x}, {width}) returned {v:#x}, which does not fit in {width} byte(s)"
                        )))
                    } else {
                        Ok(v)
                    }
                });
            value.map_err(|e| self.fail(py, e, addr))
        })
    }

    fn write(&mut self, addr: Reg, width: u32, value: u32) -> Result<(), crate::BusError> {
        Python::attach(|py| {
            self.obj
                .call_method1(py, "write", (addr, width, value))
                .map(drop)
                .map_err(|e| self.fail(py, e, addr))
        })
    }
}

impl BusDevice for PyDevice {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, crate::BusError> {
        self.read(addr, 1).map(|v| v as u8)
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, crate::BusError> {
        self.read(addr, 2).map(|v| v as u16)
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, crate::BusError> {
        self.read(addr, 4)
    }
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), crate::BusError> {
        self.write(addr, 1, val.into())
    }
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), crate::BusError> {
        self.write(addr, 2, val.into())
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), crate::BusError> {
        self.write(addr, 4, val)
    }
    fn tick(&mut self, cycles: u64) {
        if !self.has_tick || self.error.borrow().is_some() {
            return;
        }
        Python::attach(|py| {
            if let Err(e) = self.obj.call_method1(py, "tick", (cycles,)) {
                self.error.borrow_mut().get_or_insert(e);
            }
        });
    }
    fn irq_lines(&self) -> Reg {
        if !self.has_irq_lines || self.error.borrow().is_some() {
            return 0;
        }
        Python::attach(|py| {
            match self
                .obj
                .call_method0(py, "irq_lines")
                .and_then(|v| v.extract::<Reg>(py))
            {
                Ok(lines) => lines,
                Err(e) => {
                    self.error.borrow_mut().get_or_insert(e);
                    0
                }
            }
        })
    }
}

/// One entry of `Emulator.devices`.
#[derive(Clone)]
struct MappedDevice {
    base: Reg,
    size: Reg,
    name: String,
}

// --- Emulator ----------------------------------------------------------------

/// A RISC-V MCU: CPU and bus with RAM and an optional CLINT; further devices
/// (native modules or Python objects) are attached with `map_device`.
#[pyclass(unsendable)]
pub struct Emulator {
    machine: Machine,
    symbols: Option<ElfImage>,
    ram_base: Reg,
    clint_base: Option<Reg>,
    devices: Vec<MappedDevice>,
    device_error: ErrorSlot,
}

impl Emulator {
    fn map(
        &mut self,
        base: Reg,
        size: Reg,
        name: String,
        device: Box<dyn BusDevice>,
    ) -> PyResult<()> {
        if u64::from(base) + u64::from(size) > 1 << 32 {
            return Err(PyValueError::new_err(format!(
                "device window 0x{base:08X} + 0x{size:X} exceeds the 32-bit address space"
            )));
        }
        self.machine
            .bus
            .map_device(base, size, device)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        self.devices.push(MappedDevice { base, size, name });
        self.devices.sort_by_key(|d| d.base);
        Ok(())
    }

    /// Re-raises the first error of a Python device callback, if any.
    fn check_devices(&self) -> PyResult<()> {
        match self.device_error.borrow_mut().take() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Bus access from the host: a device callback error wins over the bus
    /// error it caused.
    fn host<T>(&self, result: Result<T, crate::BusError>) -> PyResult<T> {
        self.check_devices()?;
        result.map_err(bus_err)
    }
}

#[pymethods]
impl Emulator {
    #[new]
    #[pyo3(signature = (ram_base = 0x8000_0000, ram_size = 64 * 1024, *, clint_base = Some(0x0200_0000)))]
    fn new(ram_base: Reg, ram_size: usize, clint_base: Option<Reg>) -> PyResult<Self> {
        let mut emu = Self {
            machine: Machine::new(Cpu::new(ram_base), Bus::new()),
            symbols: None,
            ram_base,
            clint_base,
            devices: Vec::new(),
            device_error: ErrorSlot::default(),
        };
        let ram_len = window_size(ram_size)?;
        emu.map(
            ram_base,
            ram_len,
            "Ram".into(),
            Box::new(Ram::new(ram_size)),
        )?;
        if let Some(base) = clint_base {
            emu.map(base, CLINT_SIZE, "Clint".into(), Box::new(Clint::new()))?;
        }
        Ok(emu)
    }

    #[getter]
    fn ram_base(&self) -> Reg {
        self.ram_base
    }

    #[getter]
    fn clint_base(&self) -> Option<Reg> {
        self.clint_base
    }

    /// Maps a device at `base`.
    ///
    /// `device` is either a native device module (an object with a
    /// `__mrve32_device__()` method, e.g. `mrve32.Ram` or a separately
    /// installed package; its window size is fixed by the module), or any
    /// Python object with `read(offset, width) -> int` and
    /// `write(offset, width, value)`, plus optional `tick(cycles)` and
    /// `irq_lines() -> int`; its window is `size` or `device.size`.
    #[pyo3(signature = (base, device, *, size = None))]
    fn map_device(
        &mut self,
        base: Reg,
        device: &Bound<'_, PyAny>,
        size: Option<usize>,
    ) -> PyResult<()> {
        let name = device.get_type().qualname()?.to_string();
        if device.hasattr("__mrve32_device__")? {
            if size.is_some() {
                return Err(PyTypeError::new_err(
                    "size cannot be given for a native device (its window size is fixed)",
                ));
            }
            let capsule = device.call_method0("__mrve32_device__")?;
            let foreign = take_from_capsule(&capsule)?;
            let size = foreign.size();
            return self.map(base, size, name, Box::new(foreign));
        }

        for method in ["read", "write"] {
            if !device.getattr(method).is_ok_and(|m| m.is_callable()) {
                return Err(PyTypeError::new_err(format!(
                    "{name} is not a device: it has no {method}() method and no __mrve32_device__()"
                )));
            }
        }
        let size = match size {
            Some(size) => size,
            None => device
                .getattr("size")
                .and_then(|s| s.extract())
                .map_err(|_| {
                    PyTypeError::new_err(format!(
                        "{name}: pass size= or give the device a size attribute"
                    ))
                })?,
        };
        let wrapper = PyDevice {
            obj: device.clone().unbind(),
            has_tick: device.hasattr("tick")?,
            has_irq_lines: device.hasattr("irq_lines")?,
            error: self.device_error.clone(),
        };
        self.map(base, window_size(size)?, name, Box::new(wrapper))
    }

    /// Mapped devices as `(base, size, type name)`, sorted by address.
    #[getter]
    fn devices(&self) -> Vec<(Reg, Reg, String)> {
        self.devices
            .iter()
            .map(|d| (d.base, d.size, d.name.clone()))
            .collect()
    }

    // --- Execution -------------------------------------------------------

    /// Executes one clock cycle. Returns the trap taken, if any.
    fn step(&mut self) -> PyResult<Option<PyTrap>> {
        let trap = self.machine.step();
        self.check_devices()?;
        Ok(trap.map(PyTrap::from))
    }

    /// Runs up to `max_steps` cycles; stops early at a breakpoint, on a trap
    /// (if `stop_on_trap`) or when the hart is idle forever. An exception in
    /// a Python device stops the run and is re-raised.
    #[pyo3(signature = (max_steps, *, stop_on_trap = false))]
    fn run(&mut self, max_steps: u64, stop_on_trap: bool) -> PyResult<PyRunResult> {
        let error = self.device_error.clone();
        let res = self
            .machine
            .run_until(max_steps, stop_on_trap, || error.borrow().is_some());
        self.check_devices()?;
        let (reason, trap) = match res.reason {
            StopReason::MaxSteps => ("max_steps", None),
            StopReason::Breakpoint(_) => ("breakpoint", None),
            StopReason::Trap(trap) => ("trap", Some(trap.into())),
            StopReason::Idle => ("idle", None),
            StopReason::Aborted => ("aborted", None),
        };
        Ok(PyRunResult {
            steps: res.steps,
            reason,
            trap,
        })
    }

    /// Advances the devices by `cycles` without executing instructions
    /// (for driving peripherals directly from Python).
    fn tick(&mut self, cycles: u64) -> PyResult<()> {
        self.machine.bus.tick(cycles);
        self.check_devices()
    }

    fn add_breakpoint(&mut self, pc: Reg) {
        self.machine.add_breakpoint(pc);
    }

    fn remove_breakpoint(&mut self, pc: Reg) -> bool {
        self.machine.remove_breakpoint(pc)
    }

    fn clear_breakpoints(&mut self) {
        self.machine.clear_breakpoints();
    }

    #[getter]
    fn breakpoints(&self) -> Vec<Reg> {
        let mut bps: Vec<Reg> = self.machine.breakpoints().collect();
        bps.sort_unstable();
        bps
    }

    // --- Loading ---------------------------------------------------------

    /// Loads an ELF file (path or bytes), sets pc to its entry point and
    /// returns the entry point. Its symbols become available via `symbol()`.
    fn load_elf(&mut self, source: &Bound<'_, PyAny>) -> PyResult<Reg> {
        let data = if let Ok(bytes) = source.extract::<PyBackedBytes>() {
            bytes.to_vec()
        } else {
            let path: PathBuf = source.extract()?;
            std::fs::read(path)?
        };
        let image = ElfImage::parse(&data).map_err(|e| ElfError::new_err(e.to_string()))?;
        let loaded = self.machine.load_elf(&image);
        self.check_devices()?;
        loaded.map_err(|e| ElfError::new_err(e.to_string()))?;
        let entry = image.entry;
        self.symbols = Some(image);
        Ok(entry)
    }

    /// Address of a symbol of the last loaded ELF, or None.
    fn symbol(&self, name: &str) -> Option<Reg> {
        self.symbols.as_ref().and_then(|image| image.symbol(name))
    }

    /// All symbols of the last loaded ELF.
    #[getter]
    fn symbols(&self) -> std::collections::HashMap<String, Reg> {
        self.symbols
            .as_ref()
            .map(|image| image.symbols().clone())
            .unwrap_or_default()
    }

    /// Writes 32-bit words starting at `addr`.
    fn load_program(&mut self, addr: Reg, program: Vec<u32>) -> PyResult<()> {
        for (i, &word) in program.iter().enumerate() {
            let a = addr.wrapping_add((i * 4) as Reg);
            let res = self.machine.bus.write_u32(a, word);
            self.host(res)?;
        }
        Ok(())
    }

    // --- Memory ----------------------------------------------------------

    fn read_bytes<'py>(
        &mut self,
        py: Python<'py>,
        addr: Reg,
        size: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let mut data = Vec::with_capacity(size);
        for i in 0..size {
            let res = self.machine.bus.read_u8(addr.wrapping_add(i as Reg));
            data.push(self.host(res)?);
        }
        Ok(PyBytes::new(py, &data))
    }

    /// Writes a bytes-like object (bytes, bytearray) starting at `addr`.
    fn write_bytes(&mut self, addr: Reg, data: PyBackedBytes) -> PyResult<()> {
        for (i, &b) in data.iter().enumerate() {
            let res = self.machine.bus.write_u8(addr.wrapping_add(i as Reg), b);
            self.host(res)?;
        }
        Ok(())
    }

    fn read_u8(&mut self, addr: Reg) -> PyResult<u8> {
        let res = self.machine.bus.read_u8(addr);
        self.host(res)
    }

    fn read_u16(&mut self, addr: Reg) -> PyResult<u16> {
        let res = self.machine.bus.read_u16(addr);
        self.host(res)
    }

    fn read_u32(&mut self, addr: Reg) -> PyResult<u32> {
        let res = self.machine.bus.read_u32(addr);
        self.host(res)
    }

    fn write_u8(&mut self, addr: Reg, val: u8) -> PyResult<()> {
        let res = self.machine.bus.write_u8(addr, val);
        self.host(res)
    }

    fn write_u16(&mut self, addr: Reg, val: u16) -> PyResult<()> {
        let res = self.machine.bus.write_u16(addr, val);
        self.host(res)
    }

    fn write_u32(&mut self, addr: Reg, val: u32) -> PyResult<()> {
        let res = self.machine.bus.write_u32(addr, val);
        self.host(res)
    }

    // --- CPU state -------------------------------------------------------

    fn read_reg(&self, reg: usize) -> PyResult<Reg> {
        check_reg(reg)?;
        Ok(self.machine.cpu.read_reg(reg))
    }

    fn write_reg(&mut self, reg: usize, val: Reg) -> PyResult<()> {
        check_reg(reg)?;
        self.machine.cpu.write_reg(reg, val);
        Ok(())
    }

    /// Snapshot of x0..x31.
    #[getter]
    fn regs(&self) -> Vec<Reg> {
        (0..32).map(|r| self.machine.cpu.read_reg(r)).collect()
    }

    #[getter]
    fn pc(&self) -> Reg {
        self.machine.cpu.pc
    }

    #[setter]
    fn set_pc(&mut self, pc: Reg) {
        self.machine.cpu.pc = pc;
    }

    fn read_csr(&self, csr: u16) -> PyResult<Reg> {
        self.machine
            .cpu
            .read_csr(csr)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn write_csr(&mut self, csr: u16, val: Reg) -> PyResult<()> {
        self.machine
            .cpu
            .write_csr(csr, val)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// 64-bit `mcycle`.
    #[getter]
    fn cycles(&self) -> u64 {
        self.machine.cpu.csrs.mcycle
    }

    /// 64-bit `minstret`.
    #[getter]
    fn instret(&self) -> u64 {
        self.machine.cpu.csrs.minstret
    }

    /// True while the hart waits in `wfi`.
    #[getter]
    fn is_waiting(&self) -> bool {
        self.machine.cpu.is_waiting()
    }

    fn __repr__(&self) -> String {
        let devices: Vec<String> = self
            .devices
            .iter()
            .map(|d| format!("{}@0x{:08X}", d.name, d.base))
            .collect();
        format!(
            "Emulator(pc=0x{:08X}, devices=[{}])",
            self.machine.cpu.pc,
            devices.join(", ")
        )
    }
}

fn check_reg(reg: usize) -> PyResult<()> {
    if reg < 32 {
        Ok(())
    } else {
        Err(PyIndexError::new_err(format!(
            "register index {reg} out of range 0..32"
        )))
    }
}

/// Native module, re-exported by the `mrve32` Python package.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("DEVICE_ABI_VERSION", crate::plugin::DEVICE_ABI_VERSION)?;
    m.add_class::<Emulator>()?;
    m.add_class::<PyRam>()?;
    m.add_class::<PyClint>()?;
    m.add_class::<PyTrap>()?;
    m.add_class::<PyRunResult>()?;
    m.add("BusError", py.get_type::<BusError>())?;
    m.add("ElfError", py.get_type::<ElfError>())?;
    Ok(())
}
