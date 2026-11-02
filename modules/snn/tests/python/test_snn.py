import numpy as np
import pytest

import mrve32_snn as snn
from mrve32 import BusError, Emulator, Ram
from mrve32_snn import (
    SnnAccelerator,
    SnnDriver,
    SnnError,
    pack_spikes,
    reference_step,
    unpack_spikes,
)

SNN_BASE = 0x1000_0000


def make_device(**config):
    emu = Emulator()
    emu.map_device(SNN_BASE, SnnAccelerator(**config))
    return emu, SnnDriver(emu, SNN_BASE)


def test_pack_unpack_roundtrip():
    rng = np.random.default_rng(0)
    for n in [1, 31, 32, 33, 100]:
        bits = rng.random(n) < 0.5
        words = pack_spikes(bits)
        assert words.dtype == np.uint32 and words.size == -(-n // 32)
        assert np.array_equal(unpack_spikes(words, n), bits)
    assert pack_spikes([1, 1, 0, 1]).tolist() == [0b1011]
    assert pack_spikes([0] * 32 + [1]).tolist() == [0, 1]


def test_reference_step_matches_spec_example():
    weights = np.array(
        [[5, 12, -3], [6, -1, 4], [100, 100, 100], [0, 2, 5]], dtype=np.int8
    )
    spikes = np.array([1, 1, 0, 1], dtype=bool)
    out, vmem = reference_step(spikes, weights, np.zeros(3, np.int16), v_th=10)
    assert out.tolist() == [True, True, False]
    assert vmem.tolist() == [1, 3, 6]


def test_reference_step_validates_arguments():
    w = np.zeros((2, 3), np.int8)
    v = np.zeros(3, np.int16)
    with pytest.raises(ValueError, match="weights"):
        reference_step([1, 0], w.astype(np.int16), v, v_th=1)
    with pytest.raises(ValueError, match="vmem"):
        reference_step([1, 0], w, v.astype(np.int32), v_th=1)
    with pytest.raises(ValueError, match="v_th"):
        reference_step([1, 0], w, v, v_th=0)
    with pytest.raises(ValueError, match="leak_shift"):
        reference_step([1, 0], w, v, v_th=1, leak_shift=16)


def test_device_identification():
    emu, dev = make_device(max_inputs=200, max_outputs=100, weight_bytes=4096)
    assert (dev.max_inputs, dev.max_outputs, dev.weight_bytes) == (200, 100, 4096)
    assert (SNN_BASE, snn.WINDOW_SIZE, "SnnAccelerator") in emu.devices

    other = Emulator()
    other.map_device(SNN_BASE, Ram(4096))
    with pytest.raises(ValueError, match="no SNN"):
        SnnDriver(other, SNN_BASE)


def test_accelerator_object():
    acc = SnnAccelerator(max_inputs=16)
    assert acc.size == snn.WINDOW_SIZE
    assert "max_inputs=16" in repr(acc)
    emu = Emulator()
    emu.map_device(SNN_BASE, acc)
    with pytest.raises(RuntimeError, match="already attached"):
        Emulator().map_device(SNN_BASE, acc)
    with pytest.raises(ValueError, match="max_inputs"):
        SnnAccelerator(max_inputs=0)


@pytest.mark.parametrize("seed", range(20))
def test_emulator_matches_numpy_reference(seed):
    """Golden-model check: Rust model in the emulator == NumPy reference."""
    rng = np.random.default_rng(seed)
    n_in = int(rng.integers(1, 300))
    n_out = int(rng.integers(1, 120))
    v_th = int(rng.integers(1, 2000))
    leak_shift = int(rng.integers(0, 16))
    weights = rng.integers(-128, 128, size=(n_in, n_out), dtype=np.int8)
    bias = rng.integers(-500, 500, size=n_out, dtype=np.int16) if seed % 2 else None
    # Extreme starting potentials exercise saturation and the leak shift.
    vmem = rng.integers(-32768, 32768, size=n_out, dtype=np.int16)

    _, dev = make_device()
    dev.load_layer(weights, v_th=v_th, leak_shift=leak_shift, bias=bias)
    dev.write_vmem(vmem)

    for _ in range(5):  # several timesteps: state carries over in VMEM
        spikes = rng.random(n_in) < rng.random()
        expected_out, vmem = reference_step(
            spikes, weights, vmem, v_th=v_th, leak_shift=leak_shift, bias=bias
        )
        out = dev.step(spikes)
        assert np.array_equal(out, expected_out)
        assert np.array_equal(dev.read_vmem(), vmem)
        assert dev.in_spikes == int(spikes.sum())
        assert dev.out_spikes == int(expected_out.sum())


def test_cycles_follow_timing_model():
    _, dev = make_device(t_setup=4, t_neuron=2, t_syn=3, lanes=4)
    dev.load_layer(np.ones((16, 10), np.int8), v_th=1000)
    spikes = np.zeros(16, bool)
    spikes[:5] = True
    dev.step(spikes)
    # 4 + 10*2 + 5 spikes * ceil(10/4) * 3
    assert dev.cycles == 4 + 20 + 5 * 3 * 3


def test_sram_locked_while_busy():
    emu, dev = make_device()
    dev.load_layer(np.ones((4, 4), np.int8), v_th=1)
    dev.write_input_spikes([1, 1, 1, 1])
    dev.start()
    assert dev.busy
    with pytest.raises(BusError, match="busy"):
        dev.read_output_spikes()
    dev.wait()
    assert dev.read_output_spikes().all()


def test_configuration_errors_raise():
    _, dev = make_device(max_inputs=8, max_outputs=8, weight_bytes=16)
    dev.configure(8, 8, v_th=1)  # 64 weights > 16 bytes
    dev.start()
    with pytest.raises(SnnError) as err:
        dev.wait()
    assert err.value.code == snn.ERR_WEIGHTS_DO_NOT_FIT

    dev.configure(2, 2, v_th=0)
    dev.start()
    with pytest.raises(SnnError, match="threshold"):
        dev.wait()


def test_clear_vmem():
    _, dev = make_device()
    dev.load_layer(np.zeros((1, 3), np.int8), v_th=100)
    dev.write_vmem([5, -6, 7])
    assert dev.read_vmem().tolist() == [5, -6, 7]
    dev.clear_vmem()
    assert dev.read_vmem().tolist() == [0, 0, 0]


def test_completion_interrupt_wakes_firmware():
    """Firmware path: wfi until the accelerator's interrupt (MEIP)."""
    emu, dev = make_device()
    dev.load_layer(np.full((2, 2), 3, np.int8), v_th=5)
    dev.write_input_spikes([1, 1])
    dev.enable_irq()
    emu.load_program(emu.ram_base, [0x1050_0073, 0x0000_006F])  # wfi; j .
    emu.write_csr(0x304, 1 << 11)  # mie.MEIE, mstatus.MIE = 0: wake without trap

    dev.start()
    res = emu.run(1000, stop_on_trap=True)
    assert res.reason == "max_steps"
    assert not emu.is_waiting and emu.pc == emu.ram_base + 4
    assert dev.read_output_spikes().tolist() == [True, True]
