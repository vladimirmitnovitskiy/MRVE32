//! NS16550A-compatible UART (the subset firmware actually uses).
//!
//! The same register model as the UART of QEMU's `virt` machine, so the
//! usual 16550 drivers (Zephyr, FreeRTOS demos, bare-metal BSPs) work
//! unchanged. Transmission is instantaneous; there are no baud-rate
//! timing, FIFO depth or line-status errors.
//!
//! | offset | read | write |
//! |---|---|---|
//! | 0 | RBR: receive buffer (DLL if LCR.DLAB) | THR: transmit (DLL if LCR.DLAB) |
//! | 1 | IER (DLM if LCR.DLAB) | IER (DLM if LCR.DLAB) |
//! | 2 | IIR | FCR |
//! | 3 | LCR | LCR |
//! | 4 | MCR | MCR |
//! | 5 | LSR: DR, THRE, TEMT | ignored |
//! | 6 | MSR (0) | ignored |
//! | 7 | SCR | SCR |
//!
//! Registers are one byte apart by default; [`Uart::with_reg_shift`]
//! spaces them `1 << shift` bytes apart for 32-bit register buses. The
//! interrupt (RX data available, THR empty) drives the machine external
//! interrupt line (MEIP) directly; there is no PLIC.
//!
//! The host side is a [`UartIo`] handle, which stays usable after the
//! device has been moved onto a bus.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use crate::bus::{BusDevice, BusError};
use crate::cpu::csr::MIP_MEIP;
use crate::types::Reg;

/// Address window of the UART with the default register spacing.
pub const UART_SIZE: Reg = 0x100;

const LCR_DLAB: u8 = 1 << 7;
const IER_RDA: u8 = 1 << 0;
const IER_THRE: u8 = 1 << 1;
const LSR_DR: u8 = 1 << 0;
const LSR_THRE: u8 = 1 << 5;
const LSR_TEMT: u8 = 1 << 6;
const IIR_NONE: u8 = 0x01;
const IIR_THRE: u8 = 0x02;
const IIR_RDA: u8 = 0x04;
const IIR_FIFO: u8 = 0xC0;
const FCR_FIFO_EN: u8 = 1 << 0;
const FCR_CLEAR_RX: u8 = 1 << 1;

/// Receives every transmitted byte instead of the output buffer.
pub type UartSink = Box<dyn FnMut(u8)>;

#[derive(Default)]
struct Shared {
    tx: Vec<u8>,
    rx: VecDeque<u8>,
    sink: Option<UartSink>,
}

/// Host side of a [`Uart`]: transmitted bytes and received input.
#[derive(Clone, Default)]
pub struct UartIo(Rc<RefCell<Shared>>);

impl UartIo {
    /// Bytes transmitted so far (not cleared).
    pub fn output(&self) -> Vec<u8> {
        self.0.borrow().tx.clone()
    }

    /// Bytes transmitted since the last call; clears the buffer.
    pub fn take_output(&self) -> Vec<u8> {
        std::mem::take(&mut self.0.borrow_mut().tx)
    }

    /// Queues bytes for the firmware to receive.
    pub fn push_input(&self, data: &[u8]) {
        self.0.borrow_mut().rx.extend(data);
    }

    /// Number of received bytes the firmware has not read yet.
    pub fn pending_input(&self) -> usize {
        self.0.borrow().rx.len()
    }

    /// Sends transmitted bytes to `sink` instead of the output buffer
    /// (`None` restores buffering), e.g. to stream to stdout.
    pub fn set_sink(&self, sink: Option<UartSink>) {
        self.0.borrow_mut().sink = sink;
    }
}

impl fmt::Debug for UartIo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shared = self.0.borrow();
        f.debug_struct("UartIo")
            .field("tx_len", &shared.tx.len())
            .field("rx_len", &shared.rx.len())
            .field("sink", &shared.sink.is_some())
            .finish()
    }
}

/// NS16550A-compatible UART; see the [module docs](self).
pub struct Uart {
    io: UartIo,
    reg_shift: u32,
    ier: u8,
    lcr: u8,
    mcr: u8,
    scr: u8,
    fcr: u8,
    dll: u8,
    dlm: u8,
    /// THR-empty interrupt condition, cleared by reading IIR or writing THR
    /// and set again when the (instantaneous) transmission completes.
    thre_pending: bool,
}

impl Default for Uart {
    fn default() -> Self {
        Self::new()
    }
}

impl Uart {
    /// A UART with byte-spaced registers ([`UART_SIZE`] window).
    pub fn new() -> Self {
        Self {
            io: UartIo::default(),
            reg_shift: 0,
            ier: 0,
            lcr: 0,
            mcr: 0,
            scr: 0,
            fcr: 0,
            dll: 0,
            dlm: 0,
            thre_pending: false,
        }
    }

    /// Registers spaced `1 << shift` bytes apart (`shift` 0..=2); the
    /// window is `UART_SIZE << shift`.
    pub fn with_reg_shift(shift: u32) -> Self {
        assert!(shift <= 2, "reg_shift must be 0, 1 or 2");
        Self {
            reg_shift: shift,
            ..Self::new()
        }
    }

    /// Size of the address window.
    pub fn size(&self) -> Reg {
        UART_SIZE << self.reg_shift
    }

    /// Host-side handle (shared with the device).
    pub fn io(&self) -> UartIo {
        self.io.clone()
    }

    fn reg_index(&self, addr: Reg, width: u32) -> Result<Reg, BusError> {
        let stride = 1 << self.reg_shift;
        if !addr.is_multiple_of(stride) || width > stride {
            return Err(BusError::InvalidAccess(addr));
        }
        let index = addr >> self.reg_shift;
        if index < 8 {
            Ok(index)
        } else {
            Err(BusError::AddressNotFound(addr))
        }
    }

    fn rx_ready(&self) -> bool {
        !self.io.0.borrow().rx.is_empty()
    }

    fn iir(&self) -> u8 {
        let fifo = if self.fcr & FCR_FIFO_EN != 0 {
            IIR_FIFO
        } else {
            0
        };
        let id = if self.ier & IER_RDA != 0 && self.rx_ready() {
            IIR_RDA
        } else if self.ier & IER_THRE != 0 && self.thre_pending {
            IIR_THRE
        } else {
            IIR_NONE
        };
        fifo | id
    }

    fn read_reg(&mut self, index: Reg) -> u8 {
        let dlab = self.lcr & LCR_DLAB != 0;
        match index {
            0 if dlab => self.dll,
            0 => self.io.0.borrow_mut().rx.pop_front().unwrap_or(0),
            1 if dlab => self.dlm,
            1 => self.ier,
            2 => {
                let iir = self.iir();
                if iir & 0x0F == IIR_THRE {
                    self.thre_pending = false;
                }
                iir
            }
            3 => self.lcr,
            4 => self.mcr,
            5 => {
                let dr = if self.rx_ready() { LSR_DR } else { 0 };
                dr | LSR_THRE | LSR_TEMT
            }
            6 => 0,
            _ => self.scr,
        }
    }

    fn write_reg(&mut self, index: Reg, val: u8) {
        let dlab = self.lcr & LCR_DLAB != 0;
        match index {
            0 if dlab => self.dll = val,
            0 => {
                let mut shared = self.io.0.borrow_mut();
                match &mut shared.sink {
                    Some(sink) => sink(val),
                    None => shared.tx.push(val),
                }
                self.thre_pending = true;
            }
            1 if dlab => self.dlm = val,
            1 => {
                if val & IER_THRE != 0 && self.ier & IER_THRE == 0 {
                    self.thre_pending = true;
                }
                self.ier = val & 0x0F;
            }
            2 => {
                if val & FCR_CLEAR_RX != 0 {
                    self.io.0.borrow_mut().rx.clear();
                }
                self.fcr = val & FCR_FIFO_EN;
            }
            3 => self.lcr = val,
            4 => self.mcr = val & 0x1F,
            5 | 6 => {}
            _ => self.scr = val,
        }
    }
}

impl BusDevice for Uart {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
        let index = self.reg_index(addr, 1)?;
        Ok(self.read_reg(index))
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
        let index = self.reg_index(addr, 2)?;
        Ok(self.read_reg(index).into())
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        let index = self.reg_index(addr, 4)?;
        Ok(self.read_reg(index).into())
    }
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError> {
        let index = self.reg_index(addr, 1)?;
        self.write_reg(index, val);
        Ok(())
    }
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError> {
        let index = self.reg_index(addr, 2)?;
        self.write_reg(index, val as u8);
        Ok(())
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        let index = self.reg_index(addr, 4)?;
        self.write_reg(index, val as u8);
        Ok(())
    }
    fn irq_lines(&self) -> Reg {
        if self.iir() & IIR_NONE == 0 {
            MIP_MEIP
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THR: Reg = 0;
    const IER: Reg = 1;
    const IIR: Reg = 2;
    const LCR: Reg = 3;
    const LSR: Reg = 5;
    const SCR: Reg = 7;

    #[test]
    fn test_transmit_and_status() {
        let mut uart = Uart::new();
        let io = uart.io();
        for &b in b"hi\n" {
            assert_ne!(uart.read_u8(LSR).unwrap() & LSR_THRE, 0);
            uart.write_u8(THR, b).unwrap();
        }
        assert_eq!(io.output(), b"hi\n");
        assert_eq!(io.take_output(), b"hi\n");
        assert!(io.output().is_empty());
        assert_eq!(uart.read_u8(LSR), Ok(LSR_THRE | LSR_TEMT));
    }

    #[test]
    fn test_receive() {
        let mut uart = Uart::new();
        let io = uart.io();
        assert_eq!(uart.read_u8(LSR).unwrap() & LSR_DR, 0);
        io.push_input(b"ok");
        assert_eq!(io.pending_input(), 2);
        assert_ne!(uart.read_u8(LSR).unwrap() & LSR_DR, 0);
        assert_eq!(uart.read_u8(0), Ok(b'o'));
        assert_eq!(uart.read_u8(0), Ok(b'k'));
        assert_eq!(uart.read_u8(LSR).unwrap() & LSR_DR, 0);
        assert_eq!(uart.read_u8(0), Ok(0), "empty RBR reads 0");
    }

    #[test]
    fn test_divisor_latch_and_scratch() {
        let mut uart = Uart::new();
        let io = uart.io();
        uart.write_u8(LCR, LCR_DLAB | 0x03).unwrap();
        uart.write_u8(0, 0x12).unwrap();
        uart.write_u8(1, 0x34).unwrap();
        assert_eq!((uart.read_u8(0), uart.read_u8(1)), (Ok(0x12), Ok(0x34)));
        assert!(io.output().is_empty(), "DLL writes are not transmitted");
        uart.write_u8(LCR, 0x03).unwrap();
        assert_eq!(uart.read_u8(IER), Ok(0));
        uart.write_u8(SCR, 0xA5).unwrap();
        assert_eq!(uart.read_u8(SCR), Ok(0xA5));
    }

    #[test]
    fn test_interrupts() {
        let mut uart = Uart::new();
        let io = uart.io();
        assert_eq!(uart.irq_lines(), 0);
        assert_eq!(uart.read_u8(IIR), Ok(IIR_NONE));

        uart.write_u8(IER, IER_RDA).unwrap();
        io.push_input(b"x");
        assert_eq!(uart.irq_lines(), MIP_MEIP);
        assert_eq!(uart.read_u8(IIR), Ok(IIR_RDA));
        uart.read_u8(0).unwrap();
        assert_eq!(uart.irq_lines(), 0, "reading RBR clears RX interrupt");

        // THRE: raised on enable, cleared by reading IIR, raised by a write.
        uart.write_u8(IER, IER_THRE).unwrap();
        assert_eq!(uart.irq_lines(), MIP_MEIP);
        assert_eq!(uart.read_u8(IIR), Ok(IIR_THRE));
        assert_eq!(uart.irq_lines(), 0);
        uart.write_u8(THR, b'a').unwrap();
        assert_eq!(uart.irq_lines(), MIP_MEIP);
    }

    #[test]
    fn test_fifo_control() {
        let mut uart = Uart::new();
        let io = uart.io();
        uart.write_u8(IIR, FCR_FIFO_EN).unwrap();
        assert_eq!(uart.read_u8(IIR), Ok(IIR_FIFO | IIR_NONE));
        io.push_input(b"abc");
        uart.write_u8(IIR, FCR_FIFO_EN | FCR_CLEAR_RX).unwrap();
        assert_eq!(io.pending_input(), 0);
    }

    #[test]
    fn test_sink() {
        let mut uart = Uart::new();
        let io = uart.io();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        io.set_sink(Some(Box::new(move |b| s.borrow_mut().push(b))));
        uart.write_u8(THR, b'z').unwrap();
        assert_eq!(*seen.borrow(), b"z");
        assert!(io.output().is_empty());
    }

    #[test]
    fn test_register_spacing() {
        let mut uart = Uart::new();
        assert_eq!(uart.read_u16(0), Err(BusError::InvalidAccess(0)));
        assert_eq!(uart.read_u8(8), Err(BusError::AddressNotFound(8)));

        let mut wide = Uart::with_reg_shift(2);
        let io = wide.io();
        assert_eq!(wide.size(), 0x400);
        wide.write_u32(0, b'w'.into()).unwrap();
        assert_eq!(io.output(), b"w");
        assert_eq!(wide.read_u32(5 * 4), Ok((LSR_THRE | LSR_TEMT).into()));
        assert_eq!(wide.read_u8(1), Err(BusError::InvalidAccess(1)));
    }
}
