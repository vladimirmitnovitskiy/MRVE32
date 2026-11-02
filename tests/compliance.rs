//! Runs the riscv-tests ELF files from `tests/isa/` (rebuild them with
//! `scripts/build-riscv-tests.sh`).
//!
//! The test environment (`env/p`) reports its result by storing to the
//! `tohost` symbol: `1` means pass, `(n << 1) | 1` means test case `n` failed.

use mrve32::{Bus, BusDevice, BusError, Cpu, ElfImage, Ram, Reg};
use std::cell::Cell;
use std::fs;
use std::path::Path;
use std::rc::Rc;

const BASE: Reg = 0x8000_0000;
const RAM_SIZE: usize = 1024 * 1024;
const MAX_STEPS: usize = 100_000;
const EXPECTED_TESTS: usize = 64; // 42 rv32ui + 8 rv32um + 14 rv32mi

/// RAM that records the first non-zero word written to `tohost`.
struct HtifRam {
    ram: Ram,
    tohost_offset: Reg,
    tohost: Rc<Cell<Option<u32>>>,
}

impl BusDevice for HtifRam {
    fn read_u8(&mut self, addr: Reg) -> Result<u8, BusError> {
        self.ram.read_u8(addr)
    }
    fn read_u16(&mut self, addr: Reg) -> Result<u16, BusError> {
        self.ram.read_u16(addr)
    }
    fn read_u32(&mut self, addr: Reg) -> Result<u32, BusError> {
        self.ram.read_u32(addr)
    }
    fn write_u8(&mut self, addr: Reg, val: u8) -> Result<(), BusError> {
        self.ram.write_u8(addr, val)
    }
    fn write_u16(&mut self, addr: Reg, val: u16) -> Result<(), BusError> {
        self.ram.write_u16(addr, val)
    }
    fn write_u32(&mut self, addr: Reg, val: u32) -> Result<(), BusError> {
        if addr == self.tohost_offset && val != 0 && self.tohost.get().is_none() {
            self.tohost.set(Some(val));
        }
        self.ram.write_u32(addr, val)
    }
}

fn run_test(elf: &[u8]) -> Result<(), String> {
    let image = ElfImage::parse(elf).expect("valid ELF");
    let tohost_addr = image.symbol("tohost").expect("symbol `tohost`");
    let mut ram = Ram::new(RAM_SIZE);
    for seg in &image.segments {
        for (i, &b) in seg.data.iter().enumerate() {
            ram.write_u8(seg.addr - BASE + i as Reg, b).unwrap();
        }
    }
    let tohost = Rc::new(Cell::new(None));
    let mut bus = Bus::new();
    bus.map_device(
        BASE,
        RAM_SIZE as Reg,
        Box::new(HtifRam {
            ram,
            tohost_offset: tohost_addr - BASE,
            tohost: Rc::clone(&tohost),
        }),
    )
    .unwrap();

    let mut cpu = Cpu::new(image.entry);
    for _ in 0..MAX_STEPS {
        // Traps are part of normal test flow and are handled by the test's
        // own trap vector.
        let _ = cpu.step(&mut bus);
        match tohost.get() {
            Some(1) => return Ok(()),
            Some(code) => return Err(format!("failed test case {}", code >> 1)),
            None => {}
        }
    }
    Err(format!(
        "no result after {MAX_STEPS} steps (pc=0x{:08x}, mcause={}, mepc=0x{:08x})",
        cpu.pc, cpu.csrs.mcause, cpu.csrs.mepc
    ))
}

#[test]
fn test_official_riscv_tests() {
    let mut entries: Vec<_> = fs::read_dir(Path::new("tests/isa"))
        .expect("Directory tests/isa must exist")
        .map(|r| r.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "elf"))
        .collect();
    entries.sort();

    assert_eq!(
        entries.len(),
        EXPECTED_TESTS,
        "Expected {EXPECTED_TESTS} riscv-tests ELF files in tests/isa"
    );

    let mut failures = Vec::new();
    for path in &entries {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let elf = fs::read(path).expect("read ELF");
        match run_test(&elf) {
            Ok(()) => println!("[PASS] {name}"),
            Err(reason) => {
                println!("[FAIL] {name}: {reason}");
                failures.push(format!("{name}: {reason}"));
            }
        }
    }

    println!(
        "\n=== riscv-tests: {}/{} passed ===",
        entries.len() - failures.len(),
        entries.len()
    );
    assert!(
        failures.is_empty(),
        "failed tests:\n{}",
        failures.join("\n")
    );
}
