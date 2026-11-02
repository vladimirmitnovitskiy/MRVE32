# Fuzzing

Coverage-guided fuzz targets for [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
(needs a nightly toolchain):

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run decode            # decoders + disassembler
cargo +nightly fuzz run cpu -- -max_len=4097   # CPU on arbitrary memory images
```

`decode` checks that the 16- and 32-bit decoders never panic, report illegal
encodings as `IllegalInstruction(raw)`, and agree with the disassembler.
`cpu` runs 2000 steps of an arbitrary RAM image (first byte selects
RV32IMAC/RV32IMA) and checks that nothing panics.

The stable test suite covers the same ground deterministically in
`tests/decoder_fuzz.rs` (all 16-bit encodings, every opcode/funct3/funct7
combination, random words, random programs). CI runs each target for a
minute every week (`.github/workflows/fuzz.yml`).
