use mrve32::{Bus, BusDevice, Cpu, Ram};
use std::fs;
use std::path::Path;

#[test]
fn test_official_rv32ui_suite() {
    let bin_dir = Path::new("tests/isa");
    let mut entries: Vec<_> = fs::read_dir(bin_dir)
        .expect("Directory tests/isa must exist")
        .map(|r| r.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "bin"))
        .collect();
    entries.sort();

    assert_eq!(entries.len(), 42, "Expected exactly 42 official RV32UI tests");

    let mut passed = 0;
    let mut failed = 0;

    for path in &entries {
        let name = path.file_stem().unwrap().to_str().unwrap();
        let binary = fs::read(path).expect("read binary");

        // Официальные тесты скомпилированы под адрес 0x8000_0000
        let mut ram = Ram::new(1024 * 1024);
        for (i, &b) in binary.iter().enumerate() {
            ram.write_u8(i as u32, b).unwrap();
        }

        let mut bus = Bus::new();
        bus.map_device(0x8000_0000, 1024 * 1024, Box::new(ram));

        let mut cpu = Cpu::new(0x8000_0000);
        let mut test_passed = false;

        for step in 0..50_000 {
            if let Err(e) = cpu.step(&mut bus) {
                eprintln!("[FAIL] {}: CPU error at step {}: {:?}, pc=0x{:08x}", name, step, e, cpu.pc);
                break;
            }

            // В riscv_test.h успешный/неуспешный тест переходит в write_tohost
            if cpu.pc == 0x8000_004c || cpu.pc == 0x8000_003c {
                let gp = cpu.read_reg(3);
                if gp == 1 {
                    test_passed = true;
                } else {
                    eprintln!("[FAIL] {} failed at testcase {} (gp={})", name, gp >> 1, gp);
                }
                break;
            }
        }

        if test_passed {
            passed += 1;
        } else {
            failed += 1;
        }
    }

    println!("\n=== RISC-V Compliance Results: {}/42 passed ===", passed);
    assert_eq!(failed, 0, "Some official RISC-V tests failed!");
}
