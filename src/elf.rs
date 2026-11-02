//! Minimal ELF32 loader for little-endian RISC-V executables.
//!
//! Loads `PT_LOAD` segments at their physical (load) address, as a flash
//! programmer would, zero-filling `p_memsz > p_filesz` (`.bss`). Symbols from
//! `.symtab` are available for locating buffers and labels in firmware.

use std::collections::HashMap;
use std::fmt;

use crate::bus::{BusDevice, BusError};
use crate::types::Reg;

const EM_RISCV: u16 = 243;
const PT_LOAD: u32 = 1;
const SHT_SYMTAB: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    /// Not an ELF file, or not ELF32 / little-endian / RISC-V.
    Unsupported(&'static str),
    /// A header, segment or section points outside the file.
    Truncated,
    /// Writing a segment to the bus failed.
    Load { addr: Reg, err: BusError },
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElfError::Unsupported(what) => write!(f, "unsupported ELF: {what}"),
            ElfError::Truncated => write!(f, "malformed ELF: data out of bounds"),
            ElfError::Load { addr, err } => {
                write!(f, "cannot load segment byte at 0x{addr:08X}: {err}")
            }
        }
    }
}

impl std::error::Error for ElfError {}

/// One loadable segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Physical (load) address.
    pub addr: Reg,
    /// File contents; the remaining `mem_size - data.len()` bytes are zero.
    pub data: Vec<u8>,
    pub mem_size: u32,
}

/// A parsed ELF executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfImage {
    pub entry: Reg,
    pub segments: Vec<Segment>,
    symbols: HashMap<String, Reg>,
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn bytes(&self, off: usize, len: usize) -> Result<&[u8], ElfError> {
        off.checked_add(len)
            .and_then(|end| self.0.get(off..end))
            .ok_or(ElfError::Truncated)
    }
    fn u16(&self, off: usize) -> Result<u16, ElfError> {
        Ok(u16::from_le_bytes(self.bytes(off, 2)?.try_into().unwrap()))
    }
    fn u32(&self, off: usize) -> Result<u32, ElfError> {
        Ok(u32::from_le_bytes(self.bytes(off, 4)?.try_into().unwrap()))
    }
}

impl ElfImage {
    pub fn parse(file: &[u8]) -> Result<Self, ElfError> {
        let r = Reader(file);
        if r.bytes(0, 4)? != b"\x7fELF" {
            return Err(ElfError::Unsupported("bad magic"));
        }
        if r.bytes(4, 1)?[0] != 1 {
            return Err(ElfError::Unsupported("not ELF32"));
        }
        if r.bytes(5, 1)?[0] != 1 {
            return Err(ElfError::Unsupported("not little-endian"));
        }
        if r.u16(0x12)? != EM_RISCV {
            return Err(ElfError::Unsupported("not a RISC-V executable"));
        }

        let entry = r.u32(0x18)?;
        let phoff = r.u32(0x1C)? as usize;
        let shoff = r.u32(0x20)? as usize;
        let phentsize = r.u16(0x2A)? as usize;
        let phnum = r.u16(0x2C)? as usize;
        let shentsize = r.u16(0x2E)? as usize;
        let shnum = r.u16(0x30)? as usize;

        let mut segments = Vec::new();
        for i in 0..phnum {
            let ph = phoff + i * phentsize;
            if r.u32(ph)? != PT_LOAD {
                continue;
            }
            let offset = r.u32(ph + 4)? as usize;
            let addr = r.u32(ph + 12)?;
            let file_size = r.u32(ph + 16)? as usize;
            let mem_size = r.u32(ph + 20)?;
            if (mem_size as usize) < file_size {
                return Err(ElfError::Truncated);
            }
            segments.push(Segment {
                addr,
                data: r.bytes(offset, file_size)?.to_vec(),
                mem_size,
            });
        }

        let mut symbols = HashMap::new();
        for i in 0..shnum {
            let sh = shoff + i * shentsize;
            if r.u32(sh + 4)? != SHT_SYMTAB {
                continue;
            }
            let sym_off = r.u32(sh + 16)? as usize;
            let sym_size = r.u32(sh + 20)? as usize;
            let strtab = shoff + r.u32(sh + 24)? as usize * shentsize;
            let str_off = r.u32(strtab + 16)? as usize;
            let str_size = r.u32(strtab + 20)? as usize;
            let strings = r.bytes(str_off, str_size)?;
            for sym in (sym_off..sym_off + sym_size).step_by(16) {
                let name_off = r.u32(sym)? as usize;
                let value = r.u32(sym + 4)?;
                let name = strings.get(name_off..).ok_or(ElfError::Truncated)?;
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
                if !name.is_empty() {
                    symbols
                        .entry(String::from_utf8_lossy(name).into_owned())
                        .or_insert(value);
                }
            }
        }

        Ok(Self {
            entry,
            segments,
            symbols,
        })
    }

    /// Address of a symbol from `.symtab`, if present.
    pub fn symbol(&self, name: &str) -> Option<Reg> {
        self.symbols.get(name).copied()
    }

    pub fn symbols(&self) -> &HashMap<String, Reg> {
        &self.symbols
    }

    /// Writes all segments to the bus (byte by byte, so any device that
    /// accepts byte writes can be a load target).
    pub fn load(&self, bus: &mut dyn BusDevice) -> Result<(), ElfError> {
        for seg in &self.segments {
            let bytes = seg
                .data
                .iter()
                .copied()
                .chain(std::iter::repeat(0))
                .take(seg.mem_size as usize);
            for (i, b) in bytes.enumerate() {
                let addr = seg.addr.wrapping_add(i as Reg);
                bus.write_u8(addr, b)
                    .map_err(|err| ElfError::Load { addr, err })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bus, Ram};

    /// Minimal ELF32: one PT_LOAD (4 bytes of data, 8 bytes in memory) and
    /// a symbol table with `start` and `buf`.
    fn tiny_elf() -> Vec<u8> {
        fn put16(f: &mut [u8], off: usize, v: u16) {
            f[off..off + 2].copy_from_slice(&v.to_le_bytes());
        }
        fn put32(f: &mut [u8], off: usize, v: u32) {
            f[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        const SHOFF: usize = 0x140;

        let mut f = vec![0u8; 0x200];
        f[..4].copy_from_slice(b"\x7fELF");
        f[4] = 1; // ELFCLASS32
        f[5] = 1; // little-endian
        put16(&mut f, 0x12, EM_RISCV);
        put32(&mut f, 0x18, 0x8000_0000); // entry
        put32(&mut f, 0x1C, 0x40); // phoff
        put32(&mut f, 0x20, SHOFF as u32);
        put16(&mut f, 0x2A, 32); // phentsize
        put16(&mut f, 0x2C, 1); // phnum
        put16(&mut f, 0x2E, 40); // shentsize
        put16(&mut f, 0x30, 3); // shnum: null, symtab, strtab

        // PT_LOAD: offset 0x80, paddr 0x8000_0000, filesz 4, memsz 8
        put32(&mut f, 0x40, PT_LOAD);
        put32(&mut f, 0x44, 0x80);
        put32(&mut f, 0x4C, 0x8000_0000);
        put32(&mut f, 0x50, 4);
        put32(&mut f, 0x54, 8);
        f[0x80..0x84].copy_from_slice(&[0x13, 0, 0, 0]); // nop

        // .strtab at 0xC0
        f[0xC0..0xCB].copy_from_slice(b"\0start\0buf\0");
        // .symtab at 0xE0: null, start = 0x8000_0000, buf = 0x8000_0004
        put32(&mut f, 0xF0, 1);
        put32(&mut f, 0xF4, 0x8000_0000);
        put32(&mut f, 0x100, 7);
        put32(&mut f, 0x104, 0x8000_0004);

        let symtab = SHOFF + 40;
        put32(&mut f, symtab + 4, SHT_SYMTAB);
        put32(&mut f, symtab + 16, 0xE0);
        put32(&mut f, symtab + 20, 48);
        put32(&mut f, symtab + 24, 2); // sh_link -> section 2
        let strtab = SHOFF + 80;
        put32(&mut f, strtab + 4, 3); // SHT_STRTAB
        put32(&mut f, strtab + 16, 0xC0);
        put32(&mut f, strtab + 20, 11);
        f
    }

    #[test]
    fn test_parse_and_load_tiny_elf() {
        let elf = ElfImage::parse(&tiny_elf()).unwrap();
        assert_eq!(elf.entry, 0x8000_0000);
        assert_eq!(elf.segments.len(), 1);
        assert_eq!(elf.symbol("start"), Some(0x8000_0000));
        assert_eq!(elf.symbol("buf"), Some(0x8000_0004));
        assert_eq!(elf.symbol("missing"), None);

        let mut bus = Bus::new();
        let mut ram = Ram::new(16);
        ram.write_u32(4, 0xFFFF_FFFF).unwrap(); // будет обнулено как .bss
        bus.map_device(0x8000_0000, 16, Box::new(ram)).unwrap();
        elf.load(&mut bus).unwrap();
        assert_eq!(bus.read_u32(0x8000_0000), Ok(0x13));
        assert_eq!(bus.read_u32(0x8000_0004), Ok(0));
    }

    #[test]
    fn test_rejects_non_riscv_and_truncated_files() {
        assert_eq!(
            ElfImage::parse(b"not an elf"),
            Err(ElfError::Unsupported("bad magic"))
        );
        let mut f = tiny_elf();
        f[0x12] = 0x3E; // x86-64
        assert_eq!(
            ElfImage::parse(&f),
            Err(ElfError::Unsupported("not a RISC-V executable"))
        );
        let f = tiny_elf();
        assert_eq!(ElfImage::parse(&f[..0x50]), Err(ElfError::Truncated));
        assert_eq!(ElfImage::parse(&f[..3]), Err(ElfError::Truncated));
    }

    #[test]
    fn test_load_reports_unmapped_address() {
        let elf = ElfImage::parse(&tiny_elf()).unwrap();
        let mut bus = Bus::new();
        assert_eq!(
            elf.load(&mut bus),
            Err(ElfError::Load {
                addr: 0x8000_0000,
                err: BusError::AddressNotFound(0x8000_0000)
            })
        );
    }
}
