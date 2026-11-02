//! Python bindings (`mrve32_snn._snn`), built with the `python` feature.
//!
//! The accelerator is handed to `mrve32.Emulator.map_device` through the
//! device C ABI (`mrve32::plugin`), like any third-party device module.

use mrve32::RawDevice;
use mrve32::plugin::python::into_capsule;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use crate::{SNN_WINDOW_SIZE, SnnAccelerator, SnnConfig, TimingModel};

/// SNN layer accelerator (spec v0); attach with
/// `emu.map_device(base, SnnAccelerator(...))`.
#[pyclass(name = "SnnAccelerator", module = "mrve32_snn", unsendable)]
pub struct PySnnAccelerator {
    device: Option<SnnAccelerator>,
    config: SnnConfig,
}

#[pymethods]
impl PySnnAccelerator {
    #[new]
    #[pyo3(signature = (
        *, max_inputs = 1024, max_outputs = 512, weight_bytes = 128 * 1024,
        t_setup = 8, t_neuron = 1, t_syn = 1, lanes = 8
    ))]
    fn new(
        max_inputs: usize,
        max_outputs: usize,
        weight_bytes: usize,
        t_setup: u64,
        t_neuron: u64,
        t_syn: u64,
        lanes: u64,
    ) -> PyResult<Self> {
        if !(1..=16384).contains(&max_inputs) || !(1..=16384).contains(&max_outputs) {
            return Err(PyValueError::new_err(
                "max_inputs and max_outputs must be in 1..=16384",
            ));
        }
        if !(1..=0x7E000).contains(&weight_bytes) {
            return Err(PyValueError::new_err("weight_bytes must be in 1..=516096"));
        }
        if lanes == 0 {
            return Err(PyValueError::new_err("lanes must be >= 1"));
        }
        let config = SnnConfig {
            max_inputs,
            max_outputs,
            weight_bytes,
            timing: TimingModel {
                t_setup,
                t_neuron,
                t_syn,
                lanes,
            },
        };
        Ok(Self {
            device: Some(SnnAccelerator::new(config)),
            config,
        })
    }

    /// Size of the address window (1 MiB).
    #[getter]
    fn size(&self) -> u32 {
        SNN_WINDOW_SIZE
    }

    fn __mrve32_device__<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.take().ok_or_else(|| {
            PyRuntimeError::new_err("the device is already attached to an emulator")
        })?;
        into_capsule(py, RawDevice::new(device, SNN_WINDOW_SIZE))
    }

    fn __repr__(&self) -> String {
        let c = &self.config;
        format!(
            "SnnAccelerator(max_inputs={}, max_outputs={}, weight_bytes={}, t_setup={}, \
             t_neuron={}, t_syn={}, lanes={})",
            c.max_inputs,
            c.max_outputs,
            c.weight_bytes,
            c.timing.t_setup,
            c.timing.t_neuron,
            c.timing.t_syn,
            c.timing.lanes
        )
    }
}

// Not `_native`: with workspace feature unification (`--all-features`) this
// crate links `mrve32` with its own `#[pymodule] _native`, and two
// `PyInit__native` symbols would collide.
#[pymodule]
fn _snn(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<PySnnAccelerator>()?;
    // Memory map (docs/snn-accelerator-v0.md), the single source of truth
    // for the Python helpers.
    let consts: &[(&str, u32)] = &[
        ("WINDOW_SIZE", SNN_WINDOW_SIZE),
        ("ID", crate::SNN_ID),
        ("REG_CTRL", crate::REG_CTRL),
        ("REG_STATUS", crate::REG_STATUS),
        ("REG_IRQ_EN", crate::REG_IRQ_EN),
        ("REG_IRQ_STATUS", crate::REG_IRQ_STATUS),
        ("REG_LAYER_SIZE", crate::REG_LAYER_SIZE),
        ("REG_NEURON_CFG", crate::REG_NEURON_CFG),
        ("REG_CYCLES", crate::REG_CYCLES),
        ("REG_IN_SPIKES", crate::REG_IN_SPIKES),
        ("REG_OUT_SPIKES", crate::REG_OUT_SPIKES),
        ("REG_ID", crate::REG_ID),
        ("REG_CAPS0", crate::REG_CAPS0),
        ("REG_CAPS1", crate::REG_CAPS1),
        ("SPIKE_IN_BASE", crate::SPIKE_IN_BASE),
        ("SPIKE_OUT_BASE", crate::SPIKE_OUT_BASE),
        ("WEIGHTS_BASE", crate::WEIGHTS_BASE),
        ("VMEM_BASE", crate::VMEM_BASE),
        ("BIAS_BASE", crate::BIAS_BASE),
        ("CTRL_START", crate::CTRL_START),
        ("CTRL_CLEAR_VMEM", crate::CTRL_CLEAR_VMEM),
        ("STATUS_BUSY", crate::STATUS_BUSY),
        ("STATUS_DONE", crate::STATUS_DONE),
        ("STATUS_ERROR", crate::STATUS_ERROR),
        ("IRQ_DONE", crate::IRQ_DONE),
    ];
    for &(name, value) in consts {
        m.add(name, value)?;
    }
    Ok(())
}
