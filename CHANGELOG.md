# Changelog

All notable changes to this project are documented in this file. The format
is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(before 1.0, a minor version bump may contain breaking changes).

The Rust crates (`mrve32`, `mrve32-snn`) and the Python packages of the same
names share version numbers. The device C ABI (`include/mrve32_device.h`) is
versioned separately by `MRVE32_DEVICE_ABI_VERSION`.

To release: rename `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD`, set the same
version in both `Cargo.toml` files, and push a `vX.Y.Z` tag (see
`.github/workflows/release.yml`).

## [Unreleased]

First public release (0.1.0).

### Added

- RV32IMAC + Zicsr + Zifencei hart, machine mode only: precise exceptions
  (`mcause`/`mepc`/`mtval`), strict CSR access checks, WARL fields,
  IALIGN 16/32 (C can be disabled at runtime, `misa.C`).
- RV32A: LR/SC with a single-hart reservation, all AMOs; misaligned atomics
  raise address-misaligned exceptions.
- RV32C (Zca) with variable-length fetch; fetch faults report the failing
  half.
- Interrupts: machine timer/software/external, `WFI` with idle detection,
  `mcycle`/`minstret` and their unprivileged shadows, `mcountinhibit`.
- Pluggable devices: the `BusDevice` trait (read/write, `tick`, IRQ lines,
  exit requests); a versioned C ABI (version 2) and `ForeignDevice` for
  devices built as separate native libraries; Python devices.
- Standard peripherals with a QEMU `virt`-like map: RAM, CLINT, NS16550A
  UART subset, `sifive,test0` test finisher.
- Coprocessor slot for custom-0/custom-1 instructions (`trait Coprocessor`,
  or a Python object via `Emulator.set_coprocessor`), with atomic register
  commit.
- `Machine`: breakpoints, tracing hook, `run_until` with stop reasons
  (step limit, breakpoint, trap, idle, exit, abort).
- ELF32 loader with symbols.
- GNU objdump-compatible disassembler (pseudo-instructions, all standard
  CSR names), checked against binutils 2.46 over the whole 16-bit space and
  random 32-bit words.
- `mrve32` command-line runner: UART console, finisher exit codes,
  `--trace`, `--stats`, `--isa rv32imac|rv32ima`, configurable memory map.
- Python package `mrve32` (abi3 wheels for CPython >= 3.10) with type stubs.
- `mrve32-snn`: SNN layer accelerator device (behavioural model, spec v0)
  as a separate crate and Python package, with a NumPy reference model.
- Verification: 75 riscv-tests (rv32ui/um/ua/uc/mi) on RV32IMAC and RV32IMA,
  exhaustive decoder tests, cargo-fuzz targets (weekly CI), interpreter
  throughput benchmark (`cargo bench --bench mips`), MSRV 1.88 checked in CI.
