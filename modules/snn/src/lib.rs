//! SNN layer accelerator for the MRVE32 emulator: MMIO front-end (see
//! `docs/snn-accelerator-v0.md`). A device module: it plugs into the
//! emulator through [`mrve32::BusDevice`] like any third-party device.
//!
//! The device owns the registers, the local SRAM, the BUSY timer and the
//! interrupt line. What a step computes is delegated to a [`SnnBackend`]
//! (by default the golden [`ReferenceLif`] model).

pub mod backend;
#[cfg(feature = "python")]
mod python;

pub use backend::{LayerBuffers, LayerConfig, ReferenceLif, SnnBackend, StepStats, TimingModel};

use mrve32::Reg;
use mrve32::cpu::csr::MIP_MEIP;
use mrve32::{BusDevice, BusError};

/// Size of the device address window, for [`mrve32::Bus::map_device`].
pub const SNN_WINDOW_SIZE: Reg = 0x10_0000;

/// `ID` register value: ASCII "SN", version 0.0.
pub const SNN_ID: u32 = 0x534E_0000;

// Register offsets (spec §3)
pub const REG_CTRL: Reg = 0x000;
pub const REG_STATUS: Reg = 0x004;
pub const REG_IRQ_EN: Reg = 0x008;
pub const REG_IRQ_STATUS: Reg = 0x00C;
pub const REG_LAYER_SIZE: Reg = 0x010;
pub const REG_NEURON_CFG: Reg = 0x014;
pub const REG_CYCLES: Reg = 0x018;
pub const REG_IN_SPIKES: Reg = 0x01C;
pub const REG_OUT_SPIKES: Reg = 0x020;
pub const REG_ID: Reg = 0x0F0;
pub const REG_CAPS0: Reg = 0x0F4;
pub const REG_CAPS1: Reg = 0x0F8;

// Region base offsets (spec §2)
pub const SPIKE_IN_BASE: Reg = 0x0_1000;
pub const SPIKE_OUT_BASE: Reg = 0x0_1800;
pub const WEIGHTS_BASE: Reg = 0x0_2000;
pub const VMEM_BASE: Reg = 0x8_0000;
pub const BIAS_BASE: Reg = 0xC_0000;

const REGS_END: Reg = 0x1000;
const SPIKE_REGION_SIZE: Reg = 0x800;
const WEIGHTS_REGION_SIZE: Reg = VMEM_BASE - WEIGHTS_BASE;
const NEURON_REGION_SIZE: Reg = 0x4_0000;

pub const CTRL_START: u32 = 1 << 0;
pub const CTRL_CLEAR_VMEM: u32 = 1 << 1;
pub const STATUS_BUSY: u32 = 1 << 0;
pub const STATUS_DONE: u32 = 1 << 1;
pub const STATUS_ERROR: u32 = 1 << 2;
pub const IRQ_DONE: u32 = 1 << 0;

const NEURON_CFG_MASK: u32 = 0x010F_FFFF;
const NEURON_CFG_BIAS_EN: u32 = 1 << 24;

/// `STATUS.ERR_CODE` values (spec §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SnnError {
    InvalidLayerSize = 1,
    WeightsDoNotFit = 2,
    InvalidThreshold = 3,
}

/// Capacity and timing of one accelerator instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnnConfig {
    /// MAX_IN, 1..=16384.
    pub max_inputs: usize,
    /// MAX_OUT, 1..=16384.
    pub max_outputs: usize,
    /// WEIGHT_BYTES, 1..=0x7E000.
    pub weight_bytes: usize,
    pub timing: TimingModel,
}

impl Default for SnnConfig {
    /// 1024 inputs, 512 outputs, 128 KiB of weights.
    fn default() -> Self {
        Self {
            max_inputs: 1024,
            max_outputs: 512,
            weight_bytes: 128 * 1024,
            timing: TimingModel::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    SpikeIn,
    SpikeOut,
    Weights,
    Vmem,
    Bias,
}

pub struct SnnAccelerator {
    config: SnnConfig,
    backend: Box<dyn SnnBackend>,

    // Registers
    status: u32,
    irq_en: u32,
    irq_status: u32,
    layer_size: u32,
    neuron_cfg: u32,
    cycles: u32,
    in_spikes: u32,
    out_spikes: u32,

    /// Cycles left until the running step completes.
    remaining: u64,
    /// Values published to CYCLES/IN_SPIKES/OUT_SPIKES on completion.
    pending: (u64, StepStats),

    // Local SRAM
    spikes_in: Vec<u32>,
    spikes_out: Vec<u32>,
    weights: Vec<i8>,
    vmem: Vec<i16>,
    bias: Vec<i16>,
}

impl SnnAccelerator {
    /// An accelerator using the golden [`ReferenceLif`] backend.
    ///
    /// # Panics
    /// If `config` exceeds the limits of the address window.
    pub fn new(config: SnnConfig) -> Self {
        Self::with_backend(config, Box::new(ReferenceLif::new()))
    }

    pub fn with_backend(config: SnnConfig, backend: Box<dyn SnnBackend>) -> Self {
        assert!(
            (1..=16384).contains(&config.max_inputs),
            "max_inputs must be in 1..=16384"
        );
        assert!(
            (1..=16384).contains(&config.max_outputs),
            "max_outputs must be in 1..=16384"
        );
        assert!(
            (1..=WEIGHTS_REGION_SIZE as usize).contains(&config.weight_bytes),
            "weight_bytes must be in 1..=0x7E000"
        );
        Self {
            config,
            backend,
            status: 0,
            irq_en: 0,
            irq_status: 0,
            layer_size: 0,
            neuron_cfg: 0,
            cycles: 0,
            in_spikes: 0,
            out_spikes: 0,
            remaining: 0,
            pending: (0, StepStats::default()),
            spikes_in: vec![0; config.max_inputs.div_ceil(32)],
            spikes_out: vec![0; config.max_outputs.div_ceil(32)],
            weights: vec![0; config.weight_bytes],
            vmem: vec![0; config.max_outputs],
            bias: vec![0; config.max_outputs],
        }
    }

    pub fn config(&self) -> &SnnConfig {
        &self.config
    }

    pub fn is_busy(&self) -> bool {
        self.status & STATUS_BUSY != 0
    }

    fn layer_config(&self) -> Result<LayerConfig, SnnError> {
        let n_in = (self.layer_size & 0xFFFF) as usize;
        let n_out = (self.layer_size >> 16) as usize;
        let v_th = self.neuron_cfg as u16 as i16;
        if n_in == 0
            || n_out == 0
            || n_in > self.config.max_inputs
            || n_out > self.config.max_outputs
        {
            return Err(SnnError::InvalidLayerSize);
        }
        if n_in * n_out > self.config.weight_bytes {
            return Err(SnnError::WeightsDoNotFit);
        }
        if v_th <= 0 {
            return Err(SnnError::InvalidThreshold);
        }
        Ok(LayerConfig {
            n_in,
            n_out,
            v_th,
            leak_shift: ((self.neuron_cfg >> 16) & 0xF) as u8,
            bias_en: self.neuron_cfg & NEURON_CFG_BIAS_EN != 0,
        })
    }

    fn start(&mut self) {
        let cfg = match self.layer_config() {
            Ok(cfg) => cfg,
            Err(err) => {
                self.status = STATUS_DONE | STATUS_ERROR | (err as u32) << 8;
                self.irq_status |= IRQ_DONE;
                self.cycles = 0;
                self.in_spikes = 0;
                self.out_spikes = 0;
                return;
            }
        };
        // The functional result is computed now; SRAM stays locked (bus
        // errors) until the modelled latency has elapsed, so this is not
        // observable by firmware.
        let stats = self.backend.step(
            &cfg,
            LayerBuffers {
                spikes_in: &self.spikes_in,
                weights: &self.weights,
                bias: &self.bias,
                vmem: &mut self.vmem,
                spikes_out: &mut self.spikes_out,
            },
        );
        let cycles = self.config.timing.cycles(&cfg, &stats);
        self.pending = (cycles, stats);
        self.remaining = cycles;
        self.status = STATUS_BUSY;
    }

    fn complete(&mut self) {
        let (cycles, stats) = self.pending;
        self.cycles = cycles.min(u32::MAX as u64) as u32;
        self.in_spikes = stats.in_spikes;
        self.out_spikes = stats.out_spikes;
        self.status = STATUS_DONE;
        self.irq_status |= IRQ_DONE;
    }

    fn read_reg(&self, offset: Reg) -> u32 {
        match offset {
            REG_STATUS => self.status,
            REG_IRQ_EN => self.irq_en,
            REG_IRQ_STATUS => self.irq_status,
            REG_LAYER_SIZE => self.layer_size,
            REG_NEURON_CFG => self.neuron_cfg,
            REG_CYCLES => self.cycles,
            REG_IN_SPIKES => self.in_spikes,
            REG_OUT_SPIKES => self.out_spikes,
            REG_ID => SNN_ID,
            REG_CAPS0 => (self.config.max_outputs as u32) << 16 | self.config.max_inputs as u32,
            REG_CAPS1 => self.config.weight_bytes as u32,
            _ => 0, // CTRL and reserved offsets read as zero
        }
    }

    fn write_reg(&mut self, offset: Reg, val: u32) {
        let busy = self.is_busy();
        match offset {
            REG_CTRL if !busy => {
                if val & CTRL_CLEAR_VMEM != 0 {
                    self.vmem.fill(0);
                }
                if val & CTRL_START != 0 {
                    self.start();
                }
            }
            REG_IRQ_EN => self.irq_en = val & IRQ_DONE,
            REG_IRQ_STATUS => self.irq_status &= !(val & IRQ_DONE),
            REG_LAYER_SIZE if !busy => self.layer_size = val,
            REG_NEURON_CFG if !busy => self.neuron_cfg = val & NEURON_CFG_MASK,
            _ => {} // read-only, reserved, or ignored while BUSY
        }
    }

    /// Maps an SRAM offset to (region, byte offset within region), checking
    /// that the whole access lies in the implemented part of the region.
    fn sram_location(&self, addr: Reg, size: Reg) -> Result<(Region, usize), BusError> {
        let (region, base, limit) = match addr {
            a if (SPIKE_IN_BASE..SPIKE_IN_BASE + SPIKE_REGION_SIZE).contains(&a) => {
                (Region::SpikeIn, SPIKE_IN_BASE, self.spikes_in.len() * 4)
            }
            a if (SPIKE_OUT_BASE..SPIKE_OUT_BASE + SPIKE_REGION_SIZE).contains(&a) => {
                (Region::SpikeOut, SPIKE_OUT_BASE, self.spikes_out.len() * 4)
            }
            a if (WEIGHTS_BASE..VMEM_BASE).contains(&a) => {
                (Region::Weights, WEIGHTS_BASE, self.weights.len())
            }
            a if (VMEM_BASE..VMEM_BASE + NEURON_REGION_SIZE).contains(&a) => {
                (Region::Vmem, VMEM_BASE, self.vmem.len() * 2)
            }
            a if (BIAS_BASE..BIAS_BASE + NEURON_REGION_SIZE).contains(&a) => {
                (Region::Bias, BIAS_BASE, self.bias.len() * 2)
            }
            _ => return Err(BusError::AddressNotFound(addr)),
        };
        let offset = (addr - base) as usize;
        if offset + size as usize > limit {
            return Err(BusError::AddressNotFound(addr));
        }
        Ok((region, offset))
    }

    fn sram_byte(&self, region: Region, k: usize) -> u8 {
        match region {
            Region::SpikeIn => (self.spikes_in[k / 4] >> (8 * (k % 4))) as u8,
            Region::SpikeOut => (self.spikes_out[k / 4] >> (8 * (k % 4))) as u8,
            Region::Weights => self.weights[k] as u8,
            Region::Vmem => (self.vmem[k / 2] as u16 >> (8 * (k % 2))) as u8,
            Region::Bias => (self.bias[k / 2] as u16 >> (8 * (k % 2))) as u8,
        }
    }

    fn set_sram_byte(&mut self, region: Region, k: usize, byte: u8) {
        fn set_u32(word: &mut u32, lane: usize, byte: u8) {
            let shift = 8 * lane;
            *word = (*word & !(0xFF << shift)) | (byte as u32) << shift;
        }
        fn set_i16(half: &mut i16, lane: usize, byte: u8) {
            let shift = 8 * lane;
            *half = ((*half as u16 & !(0xFF << shift)) | (byte as u16) << shift) as i16;
        }
        match region {
            Region::SpikeIn => set_u32(&mut self.spikes_in[k / 4], k % 4, byte),
            Region::SpikeOut => set_u32(&mut self.spikes_out[k / 4], k % 4, byte),
            Region::Weights => self.weights[k] = byte as i8,
            Region::Vmem => set_i16(&mut self.vmem[k / 2], k % 2, byte),
            Region::Bias => set_i16(&mut self.bias[k / 2], k % 2, byte),
        }
    }

    fn read_sized(&mut self, addr: Reg, size: Reg) -> Result<u32, BusError> {
        if !addr.is_multiple_of(size) {
            return Err(BusError::InvalidAccess(addr));
        }
        if addr < REGS_END {
            return if size == 4 {
                Ok(self.read_reg(addr))
            } else {
                Err(BusError::InvalidAccess(addr))
            };
        }
        let (region, offset) = self.sram_location(addr, size)?;
        if self.is_busy() {
            return Err(BusError::DeviceBusy(addr));
        }
        Ok((0..size as usize).fold(0, |val, i| {
            val | (self.sram_byte(region, offset + i) as u32) << (8 * i)
        }))
    }

    fn write_sized(&mut self, addr: Reg, size: Reg, val: u32) -> Result<(), BusError> {
        if !addr.is_multiple_of(size) {
            return Err(BusError::InvalidAccess(addr));
        }
        if addr < REGS_END {
            if size != 4 {
                return Err(BusError::InvalidAccess(addr));
            }
            self.write_reg(addr, val);
            return Ok(());
        }
        let (region, offset) = self.sram_location(addr, size)?;
        if self.is_busy() {
            return Err(BusError::DeviceBusy(addr));
        }
        for i in 0..size as usize {
            self.set_sram_byte(region, offset + i, (val >> (8 * i)) as u8);
        }
        Ok(())
    }
}

impl BusDevice for SnnAccelerator {
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
        if self.is_busy() {
            self.remaining = self.remaining.saturating_sub(cycles);
            if self.remaining == 0 {
                self.complete();
            }
        }
    }

    fn irq_lines(&self) -> Reg {
        if self.irq_en & self.irq_status & IRQ_DONE != 0 {
            MIP_MEIP
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> SnnAccelerator {
        SnnAccelerator::new(SnnConfig {
            max_inputs: 64,
            max_outputs: 40,
            weight_bytes: 256,
            timing: TimingModel::default(),
        })
    }

    /// Пример из backend-тестов: 4 входа, 3 выхода, V_TH = 10
    fn load_example(dev: &mut SnnAccelerator) {
        let weights: [i8; 12] = [5, 12, -3, 6, -1, 4, 100, 100, 100, 0, 2, 5];
        for (i, &w) in weights.iter().enumerate() {
            dev.write_u8(WEIGHTS_BASE + i as Reg, w as u8).unwrap();
        }
        dev.write_u32(SPIKE_IN_BASE, 0b1011).unwrap();
        dev.write_u32(REG_LAYER_SIZE, 3 << 16 | 4).unwrap();
        dev.write_u32(REG_NEURON_CFG, 10).unwrap();
    }

    fn run_to_completion(dev: &mut SnnAccelerator) -> u64 {
        let mut cycles = 0;
        while dev.is_busy() {
            dev.tick(1);
            cycles += 1;
        }
        cycles
    }

    #[test]
    fn test_identification_and_capabilities() {
        let mut dev = small();
        assert_eq!(dev.read_u32(REG_ID), Ok(0x534E_0000));
        assert_eq!(dev.read_u32(REG_CAPS0), Ok(40 << 16 | 64));
        assert_eq!(dev.read_u32(REG_CAPS1), Ok(256));
        assert_eq!(dev.read_u32(REG_STATUS), Ok(0));
        assert_eq!(
            dev.read_u32(0x0FC),
            Ok(0),
            "зарезервированный регистр читается как 0"
        );
    }

    #[test]
    fn test_step_runs_for_modelled_cycles_then_signals_done() {
        let mut dev = small();
        load_example(&mut dev);
        dev.write_u32(REG_IRQ_EN, IRQ_DONE).unwrap();

        dev.write_u32(REG_CTRL, CTRL_START).unwrap();
        assert_eq!(dev.read_u32(REG_STATUS), Ok(STATUS_BUSY));
        assert_eq!(dev.irq_lines(), 0);

        // 8 + 3*1 + 3 спайка * ceil(3/8) * 1 = 14
        assert_eq!(run_to_completion(&mut dev), 14);
        assert_eq!(dev.read_u32(REG_STATUS), Ok(STATUS_DONE));
        assert_eq!(dev.read_u32(REG_CYCLES), Ok(14));
        assert_eq!(dev.read_u32(REG_IN_SPIKES), Ok(3));
        assert_eq!(dev.read_u32(REG_OUT_SPIKES), Ok(2));
        assert_eq!(dev.irq_lines(), MIP_MEIP);

        assert_eq!(dev.read_u32(SPIKE_OUT_BASE), Ok(0b011));
        assert_eq!(dev.read_u16(VMEM_BASE), Ok(1));
        assert_eq!(dev.read_u16(VMEM_BASE + 2), Ok(3));
        assert_eq!(dev.read_u16(VMEM_BASE + 4), Ok(6));

        // W1C подтверждение снимает прерывание
        dev.write_u32(REG_IRQ_STATUS, IRQ_DONE).unwrap();
        assert_eq!(dev.irq_lines(), 0);
    }

    #[test]
    fn test_irq_requires_enable() {
        let mut dev = small();
        load_example(&mut dev);
        dev.write_u32(REG_CTRL, CTRL_START).unwrap();
        run_to_completion(&mut dev);

        assert_eq!(dev.read_u32(REG_IRQ_STATUS), Ok(IRQ_DONE));
        assert_eq!(dev.irq_lines(), 0);
        dev.write_u32(REG_IRQ_EN, IRQ_DONE).unwrap();
        assert_eq!(dev.irq_lines(), MIP_MEIP);
    }

    #[test]
    fn test_sram_is_locked_and_config_latched_while_busy() {
        let mut dev = small();
        load_example(&mut dev);
        dev.write_u32(REG_CTRL, CTRL_START).unwrap();

        assert_eq!(
            dev.read_u32(SPIKE_OUT_BASE),
            Err(BusError::DeviceBusy(SPIKE_OUT_BASE))
        );
        assert_eq!(
            dev.write_u8(WEIGHTS_BASE, 1),
            Err(BusError::DeviceBusy(WEIGHTS_BASE))
        );
        // Регистры доступны, но конфигурация и CTRL игнорируются
        dev.write_u32(REG_LAYER_SIZE, 0).unwrap();
        dev.write_u32(REG_NEURON_CFG, 0).unwrap();
        dev.write_u32(REG_CTRL, CTRL_CLEAR_VMEM).unwrap();
        assert_eq!(dev.read_u32(REG_LAYER_SIZE), Ok(3 << 16 | 4));
        assert_eq!(dev.read_u32(REG_NEURON_CFG), Ok(10));

        run_to_completion(&mut dev);
        assert_eq!(
            dev.read_u16(VMEM_BASE),
            Ok(1),
            "CLEAR_VMEM во время BUSY проигнорирован"
        );
    }

    #[test]
    fn test_configuration_errors() {
        let cases = [
            (4, 10, SnnError::InvalidLayerSize),            // N_OUT = 0
            (3 << 16, 10, SnnError::InvalidLayerSize),      // N_IN = 0
            (3 << 16 | 65, 10, SnnError::InvalidLayerSize), // N_IN > MAX_IN
            (41 << 16 | 1, 10, SnnError::InvalidLayerSize), // N_OUT > MAX_OUT
            (40 << 16 | 7, 10, SnnError::WeightsDoNotFit),  // 280 > 256
            (3 << 16 | 4, 0, SnnError::InvalidThreshold),
            (3 << 16 | 4, 0x8000, SnnError::InvalidThreshold), // V_TH = -32768
        ];
        for (layer_size, neuron_cfg, err) in cases {
            let mut dev = small();
            dev.write_u32(REG_IRQ_EN, IRQ_DONE).unwrap();
            dev.write_u16(VMEM_BASE, 77).unwrap();
            dev.write_u32(REG_LAYER_SIZE, layer_size).unwrap();
            dev.write_u32(REG_NEURON_CFG, neuron_cfg).unwrap();
            dev.write_u32(REG_CTRL, CTRL_START).unwrap();

            assert_eq!(
                dev.read_u32(REG_STATUS),
                Ok(STATUS_DONE | STATUS_ERROR | (err as u32) << 8),
                "layer_size={layer_size:#x} neuron_cfg={neuron_cfg:#x}"
            );
            assert!(!dev.is_busy());
            assert_eq!(dev.irq_lines(), MIP_MEIP);
            assert_eq!(
                dev.read_u16(VMEM_BASE),
                Ok(77),
                "SRAM не меняется при ошибке"
            );
        }
    }

    #[test]
    fn test_clear_vmem_and_start_in_one_write() {
        let mut dev = small();
        load_example(&mut dev);
        dev.write_u16(VMEM_BASE, 9).unwrap();
        dev.write_u32(REG_CTRL, CTRL_CLEAR_VMEM | CTRL_START)
            .unwrap();
        run_to_completion(&mut dev);
        // Очистка до шага: результат как с нулевого состояния
        assert_eq!(dev.read_u16(VMEM_BASE), Ok(1));
    }

    #[test]
    fn test_neuron_cfg_reserved_bits_read_zero() {
        let mut dev = small();
        dev.write_u32(REG_NEURON_CFG, 0xFFFF_FFFF).unwrap();
        assert_eq!(dev.read_u32(REG_NEURON_CFG), Ok(0x010F_FFFF));
    }

    #[test]
    fn test_sram_byte_lanes_and_little_endian() {
        let mut dev = small();
        dev.write_u32(VMEM_BASE, 0xBEEF_1234).unwrap();
        assert_eq!(dev.read_u16(VMEM_BASE), Ok(0x1234));
        assert_eq!(dev.read_u16(VMEM_BASE + 2), Ok(0xBEEF));
        assert_eq!(dev.read_u8(VMEM_BASE + 3), Ok(0xBE));

        dev.write_u32(WEIGHTS_BASE, 0x80FF_017F).unwrap();
        assert_eq!(dev.read_u8(WEIGHTS_BASE), Ok(0x7F));
        assert_eq!(dev.read_u8(WEIGHTS_BASE + 3), Ok(0x80));

        dev.write_u8(SPIKE_IN_BASE + 1, 0xA5).unwrap();
        assert_eq!(dev.read_u32(SPIKE_IN_BASE), Ok(0x0000_A500));
    }

    #[test]
    fn test_invalid_and_out_of_range_accesses() {
        let mut dev = small();
        // Регистры: только 32-битный доступ
        assert_eq!(
            dev.read_u16(REG_STATUS),
            Err(BusError::InvalidAccess(REG_STATUS))
        );
        // Невыровненный доступ
        assert_eq!(
            dev.read_u32(WEIGHTS_BASE + 2),
            Err(BusError::InvalidAccess(WEIGHTS_BASE + 2))
        );
        // За пределами реализованной части региона (64 входа = 2 слова)
        assert_eq!(
            dev.read_u32(SPIKE_IN_BASE + 8),
            Err(BusError::AddressNotFound(SPIKE_IN_BASE + 8))
        );
        assert_eq!(
            dev.read_u8(WEIGHTS_BASE + 256),
            Err(BusError::AddressNotFound(WEIGHTS_BASE + 256))
        );
        // Последний байт региона доступен
        assert_eq!(dev.read_u8(WEIGHTS_BASE + 255), Ok(0));
        // Доступ, выходящий за конец региона, тоже ошибка
        assert_eq!(
            dev.read_u32(VMEM_BASE + 78),
            Err(BusError::InvalidAccess(VMEM_BASE + 78))
        );
        assert_eq!(
            dev.read_u16(VMEM_BASE + 80),
            Err(BusError::AddressNotFound(VMEM_BASE + 80))
        );
    }

    #[test]
    fn test_custom_backend_is_used() {
        struct AllFire;
        impl SnnBackend for AllFire {
            fn step(&mut self, cfg: &LayerConfig, bufs: LayerBuffers<'_>) -> StepStats {
                bufs.spikes_out[0] = (1 << cfg.n_out) - 1;
                StepStats {
                    in_spikes: 0,
                    out_spikes: cfg.n_out as u32,
                }
            }
        }
        let mut dev = SnnAccelerator::with_backend(SnnConfig::default(), Box::new(AllFire));
        load_example(&mut dev);
        dev.write_u32(REG_CTRL, CTRL_START).unwrap();
        run_to_completion(&mut dev);
        assert_eq!(dev.read_u32(SPIKE_OUT_BASE), Ok(0b111));
        assert_eq!(dev.read_u32(REG_OUT_SPIKES), Ok(3));
    }

    #[test]
    #[should_panic(expected = "max_outputs")]
    fn test_invalid_instance_config_panics() {
        let _ = SnnAccelerator::new(SnnConfig {
            max_outputs: 0,
            ..SnnConfig::default()
        });
    }
}

// Compiles and runs the Rust examples in README.md as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;
