//! Exhaustive and randomized robustness tests for the decoders, the
//! disassembler and the CPU (stable Rust; coverage-guided fuzzing lives in
//! `fuzz/`, see `fuzz/README.md`).

use mrve32::cpu::decode::decode;
use mrve32::cpu::decode_c::decode_compressed;
use mrve32::{Bus, BusDevice, Clint, Cpu, Exception, Ram, Reg, disassemble};

/// xorshift64*: deterministic, dependency-free.
struct Rng(u64);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }
}

fn is_unknown(text: &str) -> bool {
    text.starts_with(".insn") || text == "unimp"
}

#[test]
fn test_every_16_bit_encoding() {
    let mut legal = 0;
    for half in 0..=u16::MAX {
        if half & 0b11 == 0b11 {
            continue; // 32-bit instruction space
        }
        let text = disassemble(half as u32, Some(0x8000_0000));
        match decode_compressed(half) {
            Ok(inst) => {
                legal += 1;
                assert!(!is_unknown(&text), "0x{half:04x}: {text}");
                // Display (offset form) must not panic either.
                let _ = inst.to_string();
            }
            Err(e) => {
                assert_eq!(e, Exception::IllegalInstruction(half as u32));
                assert!(is_unknown(&text), "0x{half:04x}: {text}");
            }
        }
    }
    // The RV32 Zca space: what GNU objdump 2.46 decodes for rv32imac, minus
    // the encodings the spec reserves but objdump accepts (shamt[5] = 1,
    // c.addi16sp with imm 0). Changes here need a reason.
    assert_eq!(legal, 28_823, "legal 16-bit encodings");
}

fn check_word(raw: u32) {
    let text = disassemble(raw, Some(0x8000_0000));
    match decode(raw) {
        Ok(inst) => {
            let _ = inst.to_string();
            // Executable fences with reserved fields print as .insn.
            if is_unknown(&text) {
                assert_eq!(raw & 0x7F, 0x0F, "0x{raw:08x}: {text}");
            }
        }
        Err(e) => {
            assert_eq!(e, Exception::IllegalInstruction(raw), "0x{raw:08x}");
            assert!(is_unknown(&text), "0x{raw:08x}: {text}");
        }
    }
}

#[test]
fn test_32_bit_encodings() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    // Every major opcode x funct3 x funct7, with random other fields.
    for opcode in (0..128u32).filter(|o| o & 0b11 == 0b11) {
        for funct3 in 0..8u32 {
            for funct7 in 0..128u32 {
                for _ in 0..4 {
                    let fields = rng.next_u32() & 0x01F0_0F80 | rng.next_u32() & 0x000F_8000;
                    check_word(funct7 << 25 | fields | funct3 << 12 | opcode);
                }
            }
        }
    }
    for _ in 0..1_000_000 {
        check_word(rng.next_u32() | 0b11);
    }
}

#[test]
fn test_cpu_survives_random_programs() {
    const BASE: Reg = 0x8000_0000;
    const SIZE: usize = 16 * 1024;
    for seed in 1..=20u64 {
        let mut rng = Rng(seed.wrapping_mul(0xA076_1D64_78BD_642F) | 1);
        let mut ram = Ram::new(SIZE);
        for i in 0..SIZE / 4 {
            ram.write_u32((i * 4) as Reg, rng.next_u32()).unwrap();
        }
        let mut bus = Bus::new();
        bus.map_device(BASE, SIZE as Reg, Box::new(ram)).unwrap();
        bus.map_device(0x0200_0000, 0x10000, Box::new(Clint::new()))
            .unwrap();
        let mut cpu = Cpu::new(BASE);
        cpu.set_compressed(seed % 2 == 0);
        for _ in 0..20_000 {
            if cpu.step(&mut bus).is_some() && !(BASE..BASE + SIZE as Reg).contains(&cpu.pc) {
                // Trap vector outside RAM: re-enter at a random place.
                cpu.pc = BASE + ((rng.next_u32() % SIZE as u32) & !1);
                cpu.csrs.mtvec = BASE + ((rng.next_u32() % SIZE as u32) & !3);
            }
        }
    }
}
