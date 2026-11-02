//! End-to-end: firmware on the CPU drives the SNN accelerator over MMIO,
//! sleeps in `wfi` until the completion interrupt (MEIP) and reads the
//! output spikes.

use mrve32::{Bus, BusDevice, Cpu, Ram, Reg};
use mrve32_snn::{
    REG_LAYER_SIZE, REG_NEURON_CFG, SNN_WINDOW_SIZE, SPIKE_IN_BASE, SnnAccelerator, SnnConfig,
    VMEM_BASE, WEIGHTS_BASE,
};

const BASE: Reg = 0x8000_0000;
const SNN_BASE: Reg = 0x1000_0000;
const DONE: Reg = 0x8000_0048;

// Assembled with riscv64-linux-gnu-gcc -march=rv32im_zicsr -mabi=ilp32
const FIRMWARE: [u32; 19] = [
    0x1000_02b7, // 80000000 _start: lui   t0, 0x10000      # t0 = SNN_BASE
    0x0010_0313, // 80000004         li    t1, 1
    0x0062_a423, // 80000008         sw    t1, 8(t0)        # IRQ_EN = DONE_IE
    0x0000_1337, // 8000000c         lui   t1, 0x1
    0x8003_0313, // 80000010         addi  t1, t1, -2048    # t1 = 0x800
    0x3043_1073, // 80000014         csrw  mie, t1          # mie.MEIE
    0x0030_0313, // 80000018         li    t1, 3
    0x0062_a023, // 8000001c         sw    t1, 0(t0)        # CTRL = CLEAR_VMEM | START
    0x1050_0073, // 80000020 wait:   wfi
    0x0042_a383, // 80000024         lw    t2, 4(t0)        # STATUS
    0x0023_f393, // 80000028         andi  t2, t2, 2        # DONE?
    0xfe03_8ae3, // 8000002c         beqz  t2, wait
    0x0010_0313, // 80000030         li    t1, 1
    0x0062_a623, // 80000034         sw    t1, 12(t0)       # IRQ_STATUS = DONE_IP
    0x0182_a583, // 80000038         lw    a1, 24(t0)       # a1 = CYCLES
    0x1000_2e37, // 8000003c         lui   t3, 0x10002
    0x800e_0e13, // 80000040         addi  t3, t3, -2048    # t3 = SPIKE_OUT
    0x000e_2503, // 80000044         lw    a0, 0(t3)        # a0 = output spikes
    0x0000_006f, // 80000048 done:   j     done
];

#[test]
fn test_firmware_runs_one_layer_step_and_waits_for_interrupt() {
    let mut ram = Ram::new(4096);
    for (i, &word) in FIRMWARE.iter().enumerate() {
        ram.write_u32(4 * i as Reg, word).unwrap();
    }

    // Layer: 4 inputs -> 3 outputs, V_TH = 10, input spikes 0, 1, 3.
    // Synaptic currents I = [11, 13, 6] -> outputs 0 and 1 fire.
    let mut snn = SnnAccelerator::new(SnnConfig::default());
    let weights: [i8; 12] = [5, 12, -3, 6, -1, 4, 100, 100, 100, 0, 2, 5];
    for (i, &w) in weights.iter().enumerate() {
        snn.write_u8(WEIGHTS_BASE + i as Reg, w as u8).unwrap();
    }
    snn.write_u32(SPIKE_IN_BASE, 0b1011).unwrap();
    snn.write_u32(REG_LAYER_SIZE, 3 << 16 | 4).unwrap();
    snn.write_u32(REG_NEURON_CFG, 10).unwrap();
    // Stale state from a previous inference; the firmware clears it.
    snn.write_u16(VMEM_BASE, 500).unwrap();

    let mut bus = Bus::new();
    bus.map_device(BASE, 4096, Box::new(ram)).unwrap();
    bus.map_device(SNN_BASE, SNN_WINDOW_SIZE, Box::new(snn))
        .unwrap();

    let mut cpu = Cpu::new(BASE);
    let mut waited = 0;
    for _ in 0..1_000 {
        assert_eq!(cpu.step(&mut bus), None, "no trap expected");
        if cpu.is_waiting() {
            waited += 1;
        }
        if cpu.pc == DONE {
            break;
        }
    }

    assert_eq!(cpu.pc, DONE, "firmware did not finish");
    assert_eq!(cpu.read_reg(10), 0b011, "a0 = SPIKE_OUT word 0");
    // 8 + 3 neurons + 3 input spikes * ceil(3/8) = 14 cycles
    assert_eq!(cpu.read_reg(11), 14, "a1 = CYCLES");
    assert!(waited >= 10, "the hart slept in wfi while the step ran");

    let vmem: Vec<u16> = (0..3)
        .map(|j| bus.read_u16(SNN_BASE + VMEM_BASE + 2 * j).unwrap())
        .collect();
    assert_eq!(vmem, vec![1, 3, 6]);
    assert_eq!(bus.irq_lines(), 0, "interrupt acknowledged");
}
