//! End-to-end check of timer interrupts: firmware programs the CLINT, sleeps
//! in `wfi` and handles three timer interrupts before stopping.

use mrve32::devices::clint::CLINT_SIZE;
use mrve32::{Bus, BusDevice, Clint, Cpu, Interrupt, Ram, Reg, TrapCause};

const BASE: Reg = 0x8000_0000;
const CLINT_BASE: Reg = 0x0200_0000;
const DONE: Reg = 0x8000_0034;
const HANDLER: Reg = 0x8000_0038;

// Assembled with riscv64-linux-gnu-gcc -march=rv32im_zicsr -mabi=ilp32
const FIRMWARE: [u32; 20] = [
    0x0000_0297, // 80000000 _start:  auipc t0, 0x0
    0x0382_8293, // 80000004          addi  t0, t0, 56      # t0 = handler
    0x3052_9073, // 80000008          csrw  mtvec, t0
    0x0200_4337, // 8000000c          lui   t1, 0x2004      # t1 = &mtimecmp
    0x0640_0393, // 80000010          li    t2, 100
    0x0073_2023, // 80000014          sw    t2, 0(t1)
    0x0003_2223, // 80000018          sw    zero, 4(t1)
    0x0800_0293, // 8000001c          li    t0, 0x80        # mie.MTIE
    0x3042_9073, // 80000020          csrw  mie, t0
    0x3004_6073, // 80000024          csrsi mstatus, 8      # mstatus.MIE
    0x1050_0073, // 80000028 loop:    wfi
    0x0030_0e13, // 8000002c          li    t3, 3
    0xffc4_4ce3, // 80000030          blt   s0, t3, loop
    0x0000_006f, // 80000034 done:    j     done
    0x0014_0413, // 80000038 handler: addi  s0, s0, 1
    0x3420_24f3, // 8000003c          csrr  s1, mcause
    0x0003_2383, // 80000040          lw    t2, 0(t1)
    0x0643_8393, // 80000044          addi  t2, t2, 100     # mtimecmp += 100
    0x0073_2023, // 80000048          sw    t2, 0(t1)
    0x3020_0073, // 8000004c          mret
];

#[test]
fn test_firmware_handles_periodic_timer_interrupts() {
    let mut ram = Ram::new(4096);
    for (i, &word) in FIRMWARE.iter().enumerate() {
        ram.write_u32(4 * i as Reg, word).unwrap();
    }
    let mut bus = Bus::new();
    bus.map_device(BASE, 4096, Box::new(ram)).unwrap();
    bus.map_device(CLINT_BASE, CLINT_SIZE, Box::new(Clint::new()))
        .unwrap();

    let mut cpu = Cpu::new(BASE);
    let mut interrupts = Vec::new();
    for _ in 0..10_000 {
        if let Some(trap) = cpu.step(&mut bus) {
            assert_eq!(trap.cause, TrapCause::Interrupt(Interrupt::MachineTimer));
            assert_eq!(cpu.pc, HANDLER);
            interrupts.push(trap.epc);
        }
        if cpu.pc == DONE {
            break;
        }
    }

    assert_eq!(cpu.pc, DONE, "firmware did not finish");
    assert_eq!(cpu.read_reg(8), 3, "s0 counts handled interrupts");
    assert_eq!(
        cpu.read_reg(9),
        0x8000_0007,
        "s1 = mcause of a timer interrupt"
    );
    // Each interrupt wakes the hart from the wfi, so mepc is the next instruction.
    assert_eq!(interrupts, vec![0x8000_002C; 3]);

    // Interrupts fire at mtime = 100, 200, 300 (mtime advances once per step).
    let mtime_lo = bus.read_u32(CLINT_BASE + 0xBFF8).unwrap();
    assert!((300..400).contains(&mtime_lo), "mtime = {mtime_lo}");
    assert!(cpu.csrs.mcycle >= 300);
    // Only executed instructions retire, not the cycles spent in wfi.
    assert!(cpu.csrs.minstret < 50, "minstret = {}", cpu.csrs.minstret);
}
