//! Runs the riscv-tests ELF files from `tests/isa/` (rebuild them with
//! `scripts/build-riscv-tests.sh`).
//!
//! The test environment (`env/p`) reports its result by storing to the
//! `tohost` symbol: `1` means pass, `(n << 1) | 1` means test case `n` failed.

use mrve32::{Bus, BusDevice, BusError, Cpu, Ram, Reg};
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

fn u16_at(data: &[u8], off: usize) -> usize {
    u16::from_le_bytes(data[off..off + 2].try_into().unwrap()) as usize
}

fn u32_at(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
}

/// Minimal ELF32 loader: copies PT_LOAD segments into `ram` (mapped at
/// BASE) and returns (entry point, address of the `tohost` symbol).
fn load_elf(elf: &[u8], ram: &mut Ram) -> (Reg, Reg) {
    assert_eq!(&elf[..4], b"\x7fELF", "not an ELF file");
    assert_eq!(elf[4], 1, "expected ELFCLASS32");
    assert_eq!(elf[5], 1, "expected little-endian ELF");
    assert_eq!(u16_at(elf, 0x12), 243, "expected EM_RISCV");

    let entry = u32_at(elf, 0x18);
    let (phoff, shoff) = (u32_at(elf, 0x1C) as usize, u32_at(elf, 0x20) as usize);
    let (phentsize, phnum) = (u16_at(elf, 0x2A), u16_at(elf, 0x2C));
    let (shentsize, shnum) = (u16_at(elf, 0x2E), u16_at(elf, 0x30));

    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if u32_at(elf, ph) != 1 {
            continue; // not PT_LOAD
        }
        let offset = u32_at(elf, ph + 4) as usize;
        let paddr = u32_at(elf, ph + 12);
        let filesz = u32_at(elf, ph + 16) as usize;
        for (j, &b) in elf[offset..offset + filesz].iter().enumerate() {
            ram.write_u8(paddr - BASE + j as Reg, b).unwrap();
        }
    }

    // Find `tohost` in .symtab (its sh_link points to the string table).
    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        if u32_at(elf, sh + 4) != 2 {
            continue; // not SHT_SYMTAB
        }
        let (symoff, symsize) = (u32_at(elf, sh + 16) as usize, u32_at(elf, sh + 20) as usize);
        let strtab = shoff + u32_at(elf, sh + 24) as usize * shentsize;
        let stroff = u32_at(elf, strtab + 16) as usize;
        for sym in (symoff..symoff + symsize).step_by(16) {
            let name = &elf[stroff + u32_at(elf, sym) as usize..];
            if name.starts_with(b"tohost\0") {
                return (entry, u32_at(elf, sym + 4));
            }
        }
    }
    panic!("symbol `tohost` not found");
}

fn run_test(elf: &[u8]) -> Result<(), String> {
    let mut ram = Ram::new(RAM_SIZE);
    let (entry, tohost_addr) = load_elf(elf, &mut ram);
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

    let mut cpu = Cpu::new(entry);
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
