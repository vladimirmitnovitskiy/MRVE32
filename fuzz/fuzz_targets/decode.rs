//! Decoders and disassembler: no panics, consistent legality.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mrve32::cpu::decode::decode;
use mrve32::cpu::decode_c::decode_compressed;
use mrve32::{disassemble, Exception};

fuzz_target!(|data: [u8; 4]| {
    let raw = u32::from_le_bytes(data);
    if raw & 0b11 != 0b11 {
        let half = raw as u16;
        let text = disassemble(half as u32, None);
        match decode_compressed(half) {
            Ok(inst) => {
                assert!(!text.starts_with(".insn"), "{half:#06x}: {text}");
                let _ = inst.to_string();
            }
            Err(e) => assert_eq!(e, Exception::IllegalInstruction(half as u32)),
        }
    } else {
        let text = disassemble(raw, Some(0x8000_0000));
        match decode(raw) {
            Ok(inst) => {
                let _ = inst.to_string();
            }
            Err(e) => {
                assert_eq!(e, Exception::IllegalInstruction(raw));
                assert!(text.starts_with(".insn"), "{raw:#010x}: {text}");
            }
        }
    }
});
