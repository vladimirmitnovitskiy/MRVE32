//! Functional core of the SNN layer accelerator (spec §5): what one timestep
//! computes, independent of registers, bus access and timing.

/// Layer parameters latched by `CTRL.START`. Already validated by the device:
/// `1 <= n_in`, `1 <= n_out`, `n_in * n_out <= weights.len()`, `v_th > 0`,
/// `leak_shift <= 15`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerConfig {
    pub n_in: usize,
    pub n_out: usize,
    pub v_th: i16,
    pub leak_shift: u8,
    pub bias_en: bool,
}

/// Device SRAM handed to a backend for one step. Slices may be longer than
/// the layer needs (they cover the instance capacity).
pub struct LayerBuffers<'a> {
    /// Input spike bit vector, at least ⌈n_in/32⌉ words.
    pub spikes_in: &'a [u32],
    /// Input-major weights, `weights[i * n_out + j]`.
    pub weights: &'a [i8],
    /// Bias per output neuron, at least `n_out` entries.
    pub bias: &'a [i16],
    /// Membrane potential per output neuron, updated in place.
    pub vmem: &'a mut [i16],
    /// Output spike bit vector, at least ⌈n_out/32⌉ words.
    pub spikes_out: &'a mut [u32],
}

/// Activity counters of one step (exposed as `IN_SPIKES`/`OUT_SPIKES`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepStats {
    pub in_spikes: u32,
    pub out_spikes: u32,
}

/// Computes one timestep of one layer.
///
/// Implementations must be bit-exact with [`ReferenceLif`] for the outputs
/// (`vmem`, `spikes_out`, stats). The reference model is the golden model;
/// other backends (an optimized model, an RTL co-simulation) can be plugged
/// into [`super::SnnAccelerator::with_backend`].
pub trait SnnBackend {
    fn step(&mut self, cfg: &LayerConfig, bufs: LayerBuffers<'_>) -> StepStats;
}

/// Golden functional model: a direct transcription of spec §5.
#[derive(Debug, Clone, Default)]
pub struct ReferenceLif {
    /// Scratch accumulator, kept to avoid an allocation per step.
    current: Vec<i32>,
}

impl ReferenceLif {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Whether bit `k` of a packed spike vector is set.
pub fn spike_bit(spikes: &[u32], k: usize) -> bool {
    spikes[k / 32] >> (k % 32) & 1 != 0
}

impl SnnBackend for ReferenceLif {
    fn step(&mut self, cfg: &LayerConfig, bufs: LayerBuffers<'_>) -> StepStats {
        let n_out = cfg.n_out;

        // I[j] = BIAS_EN ? BIAS[j] : 0
        self.current.clear();
        if cfg.bias_en {
            self.current
                .extend(bufs.bias[..n_out].iter().map(|&b| b as i32));
        } else {
            self.current.resize(n_out, 0);
        }

        // I[j] += W[i][j] for every input spike i (full precision, no saturation)
        let mut in_spikes = 0;
        for i in (0..cfg.n_in).filter(|&i| spike_bit(bufs.spikes_in, i)) {
            in_spikes += 1;
            let row = &bufs.weights[i * n_out..(i + 1) * n_out];
            for (acc, &w) in self.current.iter_mut().zip(row) {
                *acc += w as i32;
            }
        }

        // Leak, integrate with saturation, fire, reset by subtraction
        let words = n_out.div_ceil(32);
        bufs.spikes_out[..words].fill(0);
        let mut out_spikes = 0;
        for (j, &current) in self.current.iter().enumerate() {
            let mut v = bufs.vmem[j];
            if cfg.leak_shift != 0 {
                v -= v >> cfg.leak_shift;
            }
            let mut v = (v as i32 + current).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            if v >= cfg.v_th {
                bufs.spikes_out[j / 32] |= 1 << (j % 32);
                v -= cfg.v_th;
                out_spikes += 1;
            }
            bufs.vmem[j] = v;
        }

        StepStats {
            in_spikes,
            out_spikes,
        }
    }
}

/// Analytical step latency (spec §6, non-normative).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimingModel {
    pub t_setup: u64,
    pub t_neuron: u64,
    pub t_syn: u64,
    /// Output neurons updated in parallel per input spike.
    pub lanes: u64,
}

impl Default for TimingModel {
    fn default() -> Self {
        Self {
            t_setup: 8,
            t_neuron: 1,
            t_syn: 1,
            lanes: 8,
        }
    }
}

impl TimingModel {
    /// Cycles the device stays BUSY for a step; at least 1.
    pub fn cycles(&self, cfg: &LayerConfig, stats: &StepStats) -> u64 {
        let n_out = cfg.n_out as u64;
        let rows = n_out.div_ceil(self.lanes.max(1));
        let cycles =
            self.t_setup + n_out * self.t_neuron + stats.in_spikes as u64 * rows * self.t_syn;
        cycles.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Layer {
        cfg: LayerConfig,
        spikes_in: Vec<u32>,
        weights: Vec<i8>,
        bias: Vec<i16>,
        vmem: Vec<i16>,
        spikes_out: Vec<u32>,
    }

    impl Layer {
        fn new(n_in: usize, n_out: usize, v_th: i16) -> Self {
            Self {
                cfg: LayerConfig {
                    n_in,
                    n_out,
                    v_th,
                    leak_shift: 0,
                    bias_en: false,
                },
                spikes_in: vec![0; n_in.div_ceil(32)],
                weights: vec![0; n_in * n_out],
                bias: vec![0; n_out],
                vmem: vec![0; n_out],
                spikes_out: vec![0; n_out.div_ceil(32)],
            }
        }

        fn step(&mut self) -> StepStats {
            ReferenceLif::new().step(
                &self.cfg,
                LayerBuffers {
                    spikes_in: &self.spikes_in,
                    weights: &self.weights,
                    bias: &self.bias,
                    vmem: &mut self.vmem,
                    spikes_out: &mut self.spikes_out,
                },
            )
        }
    }

    #[test]
    fn test_integrate_and_fire_with_input_major_weights() {
        let mut layer = Layer::new(4, 3, 10);
        // W[i][j], строка на каждый вход
        layer.weights = vec![
            5, 12, -3, // i0
            6, -1, 4, // i1
            100, 100, 100, // i2 (не спайкует)
            0, 2, 5, // i3
        ];
        layer.spikes_in[0] = 0b1011; // входы 0, 1, 3

        let stats = layer.step();
        // I = [11, 13, 6]
        assert_eq!(layer.spikes_out[0], 0b011);
        assert_eq!(layer.vmem, vec![1, 3, 6]); // остаток после reset-by-subtraction
        assert_eq!(
            stats,
            StepStats {
                in_spikes: 3,
                out_spikes: 2
            }
        );
    }

    #[test]
    fn test_reset_by_subtraction_keeps_residual_one_spike_per_step() {
        let mut layer = Layer::new(1, 1, 10);
        layer.weights = vec![25];
        layer.spikes_in[0] = 1;

        layer.step();
        assert_eq!(layer.spikes_out[0], 1);
        assert_eq!(
            layer.vmem[0], 15,
            "только один спайк за шаг, остаток сохраняется"
        );

        layer.spikes_in[0] = 0;
        layer.step();
        assert_eq!(layer.spikes_out[0], 1);
        assert_eq!(layer.vmem[0], 5);
    }

    #[test]
    fn test_leak_by_shift_rounds_toward_negative_infinity() {
        let mut layer = Layer::new(1, 4, i16::MAX);
        layer.cfg.leak_shift = 2;
        layer.vmem = vec![100, -100, -1, 3];

        layer.step();
        // v - (v >> 2): 100-25, -100-(-25), -1-(-1), 3-0
        assert_eq!(layer.vmem, vec![75, -75, 0, 3]);
    }

    #[test]
    fn test_leak_shift_zero_is_non_leaky() {
        let mut layer = Layer::new(1, 1, i16::MAX);
        layer.vmem = vec![1234];
        layer.step();
        assert_eq!(layer.vmem[0], 1234);
    }

    #[test]
    fn test_saturation_is_applied_once_after_full_precision_sum() {
        // 300 * 127 переполнило бы i16 при насыщении на каждом шаге,
        // но отрицательный вход возвращает сумму в диапазон
        let n_in = 301;
        let mut layer = Layer::new(n_in, 1, i16::MAX);
        layer.weights = vec![127; n_in];
        layer.weights[300] = -128;
        layer.spikes_in = vec![u32::MAX; n_in.div_ceil(32)];
        layer.vmem[0] = -32768;

        layer.step();
        // -32768 + 300*127 - 128 = 5204
        assert_eq!(layer.vmem[0], 5204);
    }

    #[test]
    fn test_saturates_at_i16_bounds() {
        let mut layer = Layer::new(2, 2, i16::MAX);
        layer.weights = vec![127, -128, 127, -128];
        layer.spikes_in[0] = 0b11;
        layer.vmem = vec![32700, -32700];

        layer.step();
        assert_eq!(layer.vmem, vec![0, -32768]); // 32767 >= V_TH: спайк, 32767-32767
        assert_eq!(layer.spikes_out[0], 0b01);
    }

    #[test]
    fn test_bias_is_added_when_enabled() {
        let mut layer = Layer::new(1, 2, 100);
        layer.bias = vec![7, -3];

        layer.step();
        assert_eq!(layer.vmem, vec![0, 0], "bias выключен");

        layer.cfg.bias_en = true;
        layer.step();
        assert_eq!(layer.vmem, vec![7, -3]);
    }

    #[test]
    fn test_unused_input_bits_ignored_and_output_padding_cleared() {
        let mut layer = Layer::new(3, 2, 1);
        layer.weights = vec![1, 1, 1, 1, 1, 1];
        layer.spikes_in[0] = u32::MAX; // биты >= N_IN игнорируются
        layer.spikes_out[0] = u32::MAX; // биты >= N_OUT должны стать 0

        let stats = layer.step();
        assert_eq!(stats.in_spikes, 3);
        assert_eq!(layer.spikes_out[0], 0b11);
    }

    #[test]
    fn test_timing_model() {
        let timing = TimingModel::default();
        let cfg = LayerConfig {
            n_in: 4,
            n_out: 20,
            v_th: 1,
            leak_shift: 0,
            bias_en: false,
        };
        let stats = StepStats {
            in_spikes: 3,
            out_spikes: 0,
        };
        // 8 + 20*1 + 3 * ceil(20/8)=3 * 1
        assert_eq!(timing.cycles(&cfg, &stats), 37);
    }
}
