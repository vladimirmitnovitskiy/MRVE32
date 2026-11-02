//! Stable C ABI for device modules built separately from the emulator.
//!
//! Inside one Rust program a device is simply a [`BusDevice`] mapped with
//! [`Bus::map_device`](crate::Bus::map_device). This module is for devices
//! that live in *another compiled artifact* — a separate Python extension
//! package, a shared library, a device written in C/C++ — where Rust trait
//! objects cannot be passed (their layout is not stable across compilers or
//! crate versions).
//!
//! Such a device is described by a [`RawDevice`]: a `#[repr(C)]` table of
//! an opaque context pointer and `extern "C"` callbacks. The C declaration
//! is in `include/mrve32_device.h`.
//!
//! - A device author exports a Rust device with [`RawDevice::new`] (or fills
//!   the table by hand in C).
//! - The emulator imports it with [`ForeignDevice::from_raw`], which
//!   implements [`BusDevice`] and destroys the device when dropped.
//! - In Python the table travels inside a `PyCapsule` named
//!   [`DEVICE_CAPSULE_NAME`], returned by the device object's
//!   `__mrve32_device__()` method (see [`python`], feature `python-device`).
//!
//! # Contract (ABI version 2)
//!
//! - All offsets are relative to the start of the device window; `width` is
//!   1, 2 or 4 bytes; values are little-endian, zero-extended in a `u32`.
//! - Callbacks return one of the `STATUS_*` codes.
//! - `tick`, `irq_lines` and `exit_request` may be NULL (no time
//!   behaviour / no interrupts / never stops the machine).
//! - `exit_request` returns 1 and stores the exit code when the device asks
//!   the machine to stop, otherwise 0.
//! - The emulator calls the callbacks from one thread at a time and calls
//!   `destroy` exactly once. Callbacks must not unwind (a Rust panic inside
//!   an exported callback aborts the process).

use std::ffi::{CStr, c_void};
use std::fmt;

use crate::bus::{BusDevice, BusError};
use crate::types::Reg;

/// Version of the [`RawDevice`] layout and contract.
///
/// Version 1 (never released) lacked `exit_request`.
pub const DEVICE_ABI_VERSION: u32 = 2;

/// Name of the `PyCapsule` that carries a `*mut RawDevice`. It stays the
/// same across ABI versions; `abi_version` tells them apart.
pub const DEVICE_CAPSULE_NAME: &CStr = c"mrve32.device";

pub const STATUS_OK: i32 = 0;
pub const STATUS_ADDRESS_NOT_FOUND: i32 = 1;
pub const STATUS_INVALID_ACCESS: i32 = 2;
pub const STATUS_BUSY: i32 = 3;

pub type ReadFn =
    unsafe extern "C" fn(ctx: *mut c_void, offset: u32, width: u32, value: *mut u32) -> i32;
pub type WriteFn =
    unsafe extern "C" fn(ctx: *mut c_void, offset: u32, width: u32, value: u32) -> i32;
pub type TickFn = unsafe extern "C" fn(ctx: *mut c_void, cycles: u64);
pub type IrqLinesFn = unsafe extern "C" fn(ctx: *const c_void) -> u32;
pub type DestroyFn = unsafe extern "C" fn(ctx: *mut c_void);
pub type ExitRequestFn = unsafe extern "C" fn(ctx: *const c_void, code: *mut u32) -> i32;

/// A device behind the C ABI. Owns the device until it is imported or
/// destroyed; it has no `Drop` of its own.
#[repr(C)]
#[derive(Debug)]
pub struct RawDevice {
    /// Must be [`DEVICE_ABI_VERSION`].
    pub abi_version: u32,
    /// Size of the device address window in bytes (non-zero).
    pub size: u32,
    /// Opaque device state, passed to every callback.
    pub ctx: *mut c_void,
    pub read: ReadFn,
    pub write: WriteFn,
    pub tick: Option<TickFn>,
    pub irq_lines: Option<IrqLinesFn>,
    pub destroy: DestroyFn,
    pub exit_request: Option<ExitRequestFn>,
}

impl RawDevice {
    /// Exports `device` with an address window of `size` bytes. The device
    /// is moved to the heap and freed by the `destroy` callback.
    pub fn new<D: BusDevice + 'static>(device: D, size: Reg) -> Self {
        Self {
            abi_version: DEVICE_ABI_VERSION,
            size,
            ctx: Box::into_raw(Box::new(device)).cast(),
            read: export_read::<D>,
            write: export_write::<D>,
            tick: Some(export_tick::<D>),
            irq_lines: Some(export_irq_lines::<D>),
            destroy: export_destroy::<D>,
            exit_request: Some(export_exit_request::<D>),
        }
    }
}

/// Why a [`RawDevice`] was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeviceAbiError {
    /// The device was built for another ABI version.
    UnsupportedVersion(u32),
    /// `size` is zero.
    ZeroSize,
    /// `ctx` is NULL (e.g. the device was already taken).
    NullContext,
}

impl fmt::Display for DeviceAbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(v) => write!(
                f,
                "device ABI version {v} is not supported (expected {DEVICE_ABI_VERSION})"
            ),
            Self::ZeroSize => write!(f, "device window size is zero"),
            Self::NullContext => write!(f, "device context is NULL (already attached?)"),
        }
    }
}

impl std::error::Error for DeviceAbiError {}

/// A device imported through the C ABI; usable like any [`BusDevice`].
pub struct ForeignDevice {
    raw: RawDevice,
}

impl ForeignDevice {
    /// Imports a device, taking ownership of it.
    ///
    /// On an error the device is destroyed, except for an unsupported ABI
    /// version, where the callbacks cannot be trusted and it is leaked.
    ///
    /// # Safety
    ///
    /// `raw` must follow the contract described in the [module docs](self),
    /// and the code behind its callbacks must stay loaded while the returned
    /// value lives.
    pub unsafe fn from_raw(raw: RawDevice) -> Result<Self, DeviceAbiError> {
        if raw.abi_version != DEVICE_ABI_VERSION {
            return Err(DeviceAbiError::UnsupportedVersion(raw.abi_version));
        }
        if raw.ctx.is_null() {
            return Err(DeviceAbiError::NullContext);
        }
        if raw.size == 0 {
            // SAFETY: valid v1 table with a live context, owned by us.
            unsafe { (raw.destroy)(raw.ctx) };
            return Err(DeviceAbiError::ZeroSize);
        }
        Ok(Self { raw })
    }

    /// Size of the device address window.
    pub fn size(&self) -> Reg {
        self.raw.size
    }

    fn read(&mut self, addr: Reg, width: u32) -> Result<u32, BusError> {
        let mut value = 0;
        // SAFETY: ctx is live and owned by self (from_raw contract).
        let status = unsafe { (self.raw.read)(self.raw.ctx, addr, width, &mut value) };
        status_to_result(status, addr).map(|()| value)
    }

    fn write(&mut self, addr: Reg, width: u32, value: u32) -> Result<(), BusError> {
        // SAFETY: as in `read`.
        let status = unsafe { (self.raw.write)(self.raw.ctx, addr, width, value) };
        status_to_result(status, addr)
    }
}

impl BusDevice for ForeignDevice {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
        self.read(addr, 1).map(|v| v as u8)
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
        self.read(addr, 2).map(|v| v as u16)
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        self.read(addr, 4)
    }
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError> {
        self.write(addr, 1, val.into())
    }
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError> {
        self.write(addr, 2, val.into())
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        self.write(addr, 4, val)
    }
    fn tick(&mut self, cycles: u64) {
        if let Some(tick) = self.raw.tick {
            // SAFETY: as in `read`.
            unsafe { tick(self.raw.ctx, cycles) }
        }
    }
    fn irq_lines(&self) -> Reg {
        match self.raw.irq_lines {
            // SAFETY: as in `read`.
            Some(irq_lines) => unsafe { irq_lines(self.raw.ctx) },
            None => 0,
        }
    }
    fn exit_request(&self) -> Option<u32> {
        let exit_request = self.raw.exit_request?;
        let mut code = 0;
        // SAFETY: as in `read`.
        let requested = unsafe { exit_request(self.raw.ctx, &mut code) };
        (requested != 0).then_some(code)
    }
}

impl Drop for ForeignDevice {
    fn drop(&mut self) {
        // SAFETY: ctx is live, owned by self, destroyed exactly once here.
        unsafe { (self.raw.destroy)(self.raw.ctx) }
    }
}

fn status_to_result(status: i32, addr: Reg) -> Result<(), BusError> {
    match status {
        STATUS_OK => Ok(()),
        STATUS_ADDRESS_NOT_FOUND => Err(BusError::AddressNotFound(addr)),
        STATUS_BUSY => Err(BusError::DeviceBusy(addr)),
        _ => Err(BusError::InvalidAccess(addr)),
    }
}

fn error_to_status(err: BusError) -> i32 {
    match err {
        BusError::AddressNotFound(_) => STATUS_ADDRESS_NOT_FOUND,
        BusError::DeviceBusy(_) => STATUS_BUSY,
        _ => STATUS_INVALID_ACCESS,
    }
}

unsafe extern "C" fn export_read<D: BusDevice>(
    ctx: *mut c_void,
    offset: u32,
    width: u32,
    value: *mut u32,
) -> i32 {
    // SAFETY: ctx was created by RawDevice::new::<D> and is still live.
    let device = unsafe { &mut *ctx.cast::<D>() };
    let result = match width {
        1 => device.read_u8(offset).map(u32::from),
        2 => device.read_u16(offset).map(u32::from),
        4 => device.read_u32(offset),
        _ => return STATUS_INVALID_ACCESS,
    };
    match result {
        Ok(v) => {
            // SAFETY: the caller passes a valid out pointer.
            unsafe { value.write(v) };
            STATUS_OK
        }
        Err(e) => error_to_status(e),
    }
}

unsafe extern "C" fn export_write<D: BusDevice>(
    ctx: *mut c_void,
    offset: u32,
    width: u32,
    value: u32,
) -> i32 {
    // SAFETY: as in export_read.
    let device = unsafe { &mut *ctx.cast::<D>() };
    let result = match width {
        1 => device.write_u8(offset, value as u8),
        2 => device.write_u16(offset, value as u16),
        4 => device.write_u32(offset, value),
        _ => return STATUS_INVALID_ACCESS,
    };
    result.map_or_else(error_to_status, |()| STATUS_OK)
}

unsafe extern "C" fn export_tick<D: BusDevice>(ctx: *mut c_void, cycles: u64) {
    // SAFETY: as in export_read.
    unsafe { &mut *ctx.cast::<D>() }.tick(cycles);
}

unsafe extern "C" fn export_irq_lines<D: BusDevice>(ctx: *const c_void) -> u32 {
    // SAFETY: as in export_read.
    unsafe { &*ctx.cast::<D>() }.irq_lines()
}

unsafe extern "C" fn export_exit_request<D: BusDevice>(ctx: *const c_void, code: *mut u32) -> i32 {
    // SAFETY: as in export_read.
    match unsafe { &*ctx.cast::<D>() }.exit_request() {
        Some(c) => {
            // SAFETY: the caller passes a valid out pointer.
            unsafe { code.write(c) };
            1
        }
        None => 0,
    }
}

unsafe extern "C" fn export_destroy<D: BusDevice>(ctx: *mut c_void) {
    // SAFETY: ctx came from Box::into_raw in RawDevice::new::<D>.
    drop(unsafe { Box::from_raw(ctx.cast::<D>()) });
}

/// Passing devices between Python extension modules (feature
/// `python-device`).
///
/// A device module's Python class implements `__mrve32_device__(self)`
/// returning [`into_capsule`](python::into_capsule)`(py, RawDevice::new(device, size))`;
/// `mrve32.Emulator.map_device` calls it and takes the device with
/// [`take_from_capsule`](python::take_from_capsule). Taking sets the capsule's `ctx` to NULL, so the
/// capsule destructor frees only devices that were never taken.
#[cfg(feature = "python-device")]
pub mod python {
    use std::ptr;

    use pyo3::exceptions::{PyTypeError, PyValueError};
    use pyo3::ffi;
    use pyo3::prelude::*;

    use super::{DEVICE_ABI_VERSION, DEVICE_CAPSULE_NAME, ForeignDevice, RawDevice};

    /// Wraps an exported device in a `mrve32.device` capsule.
    pub fn into_capsule(py: Python<'_>, raw: RawDevice) -> PyResult<Bound<'_, PyAny>> {
        let ptr = Box::into_raw(Box::new(raw));
        // SAFETY: ptr is a valid heap RawDevice; the name is 'static.
        let capsule = unsafe {
            ffi::PyCapsule_New(
                ptr.cast(),
                DEVICE_CAPSULE_NAME.as_ptr(),
                Some(capsule_destructor),
            )
        };
        if capsule.is_null() {
            // SAFETY: the capsule was not created, we still own ptr.
            let raw = unsafe { Box::from_raw(ptr) };
            unsafe { (raw.destroy)(raw.ctx) };
            return Err(PyErr::fetch(py));
        }
        // SAFETY: PyCapsule_New returned a new reference.
        Ok(unsafe { Bound::from_owned_ptr(py, capsule) })
    }

    unsafe extern "C" fn capsule_destructor(capsule: *mut ffi::PyObject) {
        // SAFETY: called by Python with our capsule.
        let ptr = unsafe { ffi::PyCapsule_GetPointer(capsule, DEVICE_CAPSULE_NAME.as_ptr()) };
        if ptr.is_null() {
            unsafe { ffi::PyErr_Clear() };
            return;
        }
        // SAFETY: the pointer was created by into_capsule.
        let raw = unsafe { Box::from_raw(ptr.cast::<RawDevice>()) };
        if !raw.ctx.is_null() {
            unsafe { (raw.destroy)(raw.ctx) };
        }
    }

    /// Takes the device out of a `mrve32.device` capsule.
    pub fn take_from_capsule(capsule: &Bound<'_, PyAny>) -> PyResult<ForeignDevice> {
        // SAFETY: GetPointer checks the object type and the name.
        let ptr =
            unsafe { ffi::PyCapsule_GetPointer(capsule.as_ptr(), DEVICE_CAPSULE_NAME.as_ptr()) }
                .cast::<RawDevice>();
        if ptr.is_null() {
            let _ = PyErr::take(capsule.py());
            // SAFETY: GetName is only called on a capsule.
            let name = unsafe {
                if ffi::PyCapsule_CheckExact(capsule.as_ptr()) != 0 {
                    let name = ffi::PyCapsule_GetName(capsule.as_ptr());
                    let _ = PyErr::take(capsule.py());
                    (!name.is_null()).then(|| std::ffi::CStr::from_ptr(name).to_owned())
                } else {
                    None
                }
            };
            if name.is_some_and(|n| n.to_bytes().starts_with(b"mrve32.device")) {
                return Err(PyValueError::new_err(
                    "the device module was built for an older mrve32 device ABI; \
                     rebuild it against this mrve32 version",
                ));
            }
            return Err(PyTypeError::new_err(format!(
                "__mrve32_device__() must return a '{}' capsule",
                DEVICE_CAPSULE_NAME.to_string_lossy()
            )));
        }
        // SAFETY: a capsule with our name holds a RawDevice whose first
        // field is the ABI version (stable across versions).
        let version = unsafe { (*ptr).abi_version };
        if version != DEVICE_ABI_VERSION {
            return Err(PyValueError::new_err(format!(
                "device ABI version {version} is not supported (expected {DEVICE_ABI_VERSION}); \
                 rebuild the device module against this mrve32 version"
            )));
        }
        // SAFETY: v1 layout checked. Copy the table out and mark the capsule
        // as taken so its destructor does not free the device.
        let raw = unsafe { ptr::read(ptr) };
        if raw.ctx.is_null() {
            return Err(PyValueError::new_err("the device was already attached"));
        }
        unsafe { (*ptr).ctx = ptr::null_mut() };
        // SAFETY: the capsule contract is the RawDevice contract; Python
        // never unloads extension modules, so the callbacks stay loaded.
        unsafe { ForeignDevice::from_raw(raw) }.map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ram;
    use std::cell::Cell;
    use std::rc::Rc;

    struct Probe {
        reg: u32,
        ticks: Rc<Cell<u64>>,
        dropped: Rc<Cell<bool>>,
    }

    impl BusDevice for Probe {
        fn read_u8(&mut self, a: Reg) -> Result<u8, BusError> {
            Err(BusError::InvalidAccess(a))
        }
        fn read_u16(&mut self, a: Reg) -> Result<u16, BusError> {
            Err(BusError::DeviceBusy(a))
        }
        fn read_u32(&mut self, a: Reg) -> Result<u32, BusError> {
            match a {
                0 => Ok(self.reg),
                _ => Err(BusError::AddressNotFound(a)),
            }
        }
        fn write_u8(&mut self, a: Reg, _: u8) -> Result<(), BusError> {
            Err(BusError::InvalidAccess(a))
        }
        fn write_u16(&mut self, a: Reg, _: u16) -> Result<(), BusError> {
            Err(BusError::InvalidAccess(a))
        }
        fn write_u32(&mut self, _: Reg, v: u32) -> Result<(), BusError> {
            self.reg = v;
            Ok(())
        }
        fn tick(&mut self, cycles: u64) {
            self.ticks.set(self.ticks.get() + cycles);
        }
        fn irq_lines(&self) -> Reg {
            self.reg & 0x800
        }
        fn exit_request(&self) -> Option<u32> {
            (self.reg == 0xDEAD).then_some(7)
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }

    #[test]
    fn test_round_trip_through_c_abi() {
        let ticks = Rc::new(Cell::new(0));
        let dropped = Rc::new(Cell::new(false));
        let probe = Probe {
            reg: 0,
            ticks: ticks.clone(),
            dropped: dropped.clone(),
        };
        let mut dev = unsafe { ForeignDevice::from_raw(RawDevice::new(probe, 0x100)) }.unwrap();
        assert_eq!(dev.size(), 0x100);

        dev.write_u32(0, 0x0800_0801).unwrap();
        assert_eq!(dev.read_u32(0), Ok(0x0800_0801));
        assert_eq!(dev.irq_lines(), 0x800);
        dev.tick(5);
        dev.tick(2);
        assert_eq!(ticks.get(), 7);
        assert_eq!(dev.exit_request(), None);
        dev.write_u32(0, 0xDEAD).unwrap();
        assert_eq!(dev.exit_request(), Some(7));

        // Errors keep their kind across the ABI.
        assert_eq!(dev.read_u32(4), Err(BusError::AddressNotFound(4)));
        assert_eq!(dev.read_u16(2), Err(BusError::DeviceBusy(2)));
        assert_eq!(dev.read_u8(1), Err(BusError::InvalidAccess(1)));

        assert!(!dropped.get());
        drop(dev);
        assert!(dropped.get(), "destroy callback frees the device");
    }

    #[test]
    fn test_foreign_device_on_bus() {
        let raw = RawDevice::new(Ram::new(16), 16);
        let dev = unsafe { ForeignDevice::from_raw(raw) }.unwrap();
        let mut bus = crate::Bus::new();
        bus.map_device(0x1000, dev.size(), Box::new(dev)).unwrap();
        bus.write_u16(0x1002, 0xBEEF).unwrap();
        assert_eq!(bus.read_u32(0x1000), Ok(0xBEEF_0000));
        assert_eq!(bus.read_u8(0x1003), Ok(0xBE));
    }

    #[test]
    fn test_rejects_bad_tables() {
        let mut raw = RawDevice::new(Ram::new(4), 4);
        raw.abi_version = 99;
        assert_eq!(
            unsafe { ForeignDevice::from_raw(raw) }.err(),
            Some(DeviceAbiError::UnsupportedVersion(99))
        );

        let dropped = Rc::new(Cell::new(false));
        let probe = Probe {
            reg: 0,
            ticks: Rc::default(),
            dropped: dropped.clone(),
        };
        let raw = RawDevice::new(probe, 0);
        assert_eq!(
            unsafe { ForeignDevice::from_raw(raw) }.err(),
            Some(DeviceAbiError::ZeroSize)
        );
        assert!(dropped.get(), "a rejected device is destroyed");
    }

    /// Keep in sync with `include/mrve32_device.h`.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn test_layout_matches_c_header() {
        use std::mem::{offset_of, size_of};
        assert_eq!(offset_of!(RawDevice, abi_version), 0);
        assert_eq!(offset_of!(RawDevice, size), 4);
        assert_eq!(offset_of!(RawDevice, ctx), 8);
        assert_eq!(offset_of!(RawDevice, read), 16);
        assert_eq!(offset_of!(RawDevice, write), 24);
        assert_eq!(offset_of!(RawDevice, tick), 32);
        assert_eq!(offset_of!(RawDevice, irq_lines), 40);
        assert_eq!(offset_of!(RawDevice, destroy), 48);
        assert_eq!(offset_of!(RawDevice, exit_request), 56);
        assert_eq!(size_of::<RawDevice>(), 64);
    }

    #[test]
    fn test_optional_callbacks() {
        let mut raw = RawDevice::new(Ram::new(4), 4);
        raw.tick = None;
        raw.irq_lines = None;
        raw.exit_request = None;
        let mut dev = unsafe { ForeignDevice::from_raw(raw) }.unwrap();
        dev.tick(10);
        assert_eq!(dev.irq_lines(), 0);
        assert_eq!(dev.exit_request(), None);
    }
}
