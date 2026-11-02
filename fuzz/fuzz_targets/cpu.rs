//! The CPU on arbitrary memory contents: no panics, no hangs, RV32IMAC and
//! RV32IMA, 2000 steps. The input is the RAM image (the program and its data).
#![no_main]

use libfuzzer_sys::fuzz_target;
use mrve32::{Bus, BusDevice, Clint, Cpu, Ram, Reg};

const BASE: Reg = 0x8000_0000;
const SIZE: usize = 4096;

fuzz_target!(|data: &[u8]| {
    let Some((&flags, image)) = data.split_first() else {
        return;
    };
    let mut ram = Ram::new(SIZE);
    for (i, &b) in image.iter().take(SIZE).enumerate() {
        ram.write_u8(i as Reg, b).unwrap();
    }
    let mut bus = Bus::new();
    bus.map_device(BASE, SIZE as Reg, Box::new(ram)).unwrap();
    bus.map_device(0x0200_0000, 0x10000, Box::new(Clint::new()))
        .unwrap();
    let mut cpu = Cpu::new(BASE);
    cpu.set_compressed(flags & 1 == 0);
    cpu.csrs.mtvec = BASE; // traps restart the image
    for _ in 0..2_000 {
        cpu.step(&mut bus);
        bus.tick(1);
    }
});
