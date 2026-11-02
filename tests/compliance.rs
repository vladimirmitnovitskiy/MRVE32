//! Runs the riscv-tests ELF files from `tests/isa/` (rebuild them with
//! `scripts/build-riscv-tests.sh`).
//!
//! The test environment (`env/p`) reports its result through HTIF `tohost`
//! (see `mrve32::Htif`): exit code 0 means pass, `n` means test case `n`
//! failed.

use mrve32::{Bus, BusDevice, Cpu, ElfImage, Htif, Ram, Reg};
use std::fs;
use std::path::Path;

const BASE: Reg = 0x8000_0000;
const RAM_SIZE: usize = 1024 * 1024;
const MAX_STEPS: usize = 100_000;
const EXPECTED_TESTS: usize = 75; // 42 rv32ui + 8 rv32um + 10 rv32ua + 14 rv32mi + 1 rv32uc

/// Runs one test; `compressed` selects RV32IMC or RV32IM (IALIGN 16/32).
fn run_test(elf: &[u8], compressed: bool) -> Result<(), String> {
    let image = ElfImage::parse(elf).expect("valid ELF");
    let tohost_addr = image.symbol("tohost").expect("symbol `tohost`");
    let mut ram = Ram::new(RAM_SIZE);
    for seg in &image.segments {
        for (i, &b) in seg.data.iter().enumerate() {
            ram.write_u8(seg.addr - BASE + i as Reg, b).unwrap();
        }
    }
    let mut bus = Bus::new();
    bus.map_device(
        BASE,
        RAM_SIZE as Reg,
        Box::new(Htif::new(ram, tohost_addr - BASE)),
    )
    .unwrap();

    let mut cpu = Cpu::new(image.entry);
    cpu.set_compressed(compressed);
    for _ in 0..MAX_STEPS {
        // Traps are part of normal test flow and are handled by the test's
        // own trap vector.
        let _ = cpu.step(&mut bus);
        match bus.exit_request() {
            Some(0) => return Ok(()),
            Some(case) => return Err(format!("failed test case {case}")),
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

    // Every test runs on RV32IMAC; the ones that do not use C also run on
    // RV32IMA, which exercises IALIGN = 32 (e.g. in rv32mi-p-ma_fetch).
    let mut runs = 0;
    let mut failures = Vec::new();
    for path in &entries {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let elf = fs::read(path).expect("read ELF");
        let isas: &[(&str, bool)] = if name.starts_with("rv32uc-") {
            &[("rv32imac", true)]
        } else {
            &[("rv32imac", true), ("rv32ima", false)]
        };
        for &(isa, compressed) in isas {
            runs += 1;
            match run_test(&elf, compressed) {
                Ok(()) => println!("[PASS] {name} ({isa})"),
                Err(reason) => {
                    println!("[FAIL] {name} ({isa}): {reason}");
                    failures.push(format!("{name} ({isa}): {reason}"));
                }
            }
        }
    }

    println!(
        "\n=== riscv-tests: {}/{runs} runs passed ({} ELF files) ===",
        runs - failures.len(),
        entries.len()
    );
    assert!(
        failures.is_empty(),
        "failed tests:\n{}",
        failures.join("\n")
    );
}
