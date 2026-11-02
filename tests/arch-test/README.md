# riscv-arch-test (ACT4) on MRVE32

The [RISC-V Architectural Certification Tests](https://github.com/riscv-non-isa/riscv-arch-test)
(ACT4 framework, pinned to revision `2dc36dd2`) are assembly tests that check
results at every *signature point*. The framework compiles each test twice:
a signature ELF whose results are computed by a reference model, then a
self-checking ELF with those values built in, which the device under test
runs and which prints `RVCP-SUMMARY: TEST PASSED` or the failing instruction,
register, expected and actual value.

`mrve32-rv32imac/` is the device configuration for the `mrve32` CLI with its
default memory map:

| File | Contents |
|---|---|
| `test_config.yaml` | framework configuration (compiler, Sail, files below) |
| `mrve32-rv32imac.yaml` | UDB configuration: RV32IMAC_Zicsr_Zifencei_Zicntr_Zihintntl_Zihintpause, Sm 1.12, M-mode only, no PMP, no HPM counters, no `time` CSR |
| `sail.json` | the same machine for the Sail reference model |
| `rvmodel_macros.h` | halt through the test finisher (`0x100000`), console on the NS16550 UART, CLINT timer/MSIP, external interrupt from the UART |
| `link.ld` | 16 MiB at `0x8000_0000` |

There are two ways to run them (`scripts/arch-test.sh`):

## With Sail (certification flow)

Needs the ACT4 tools (mise with uv and Ruby/UDB, Sail 0.16, a RISC-V GCC 15+;
see the riscv-arch-test README). In a riscv-arch-test checkout:

```bash
CONFIG_FILES=/path/to/MRVE32/tests/arch-test/mrve32-rv32imac/test_config.yaml make elfs
/path/to/MRVE32/scripts/arch-test.sh run work/mrve32-rv32imac/elfs
```

This includes the privileged (Sm) tests. CI runs it on every push to `main`
and weekly (`.github/workflows/arch-test.yml`, job `sail`).

## Without Sail (differential flow)

```bash
scripts/arch-test.sh local --qemu        # or: local --qemu I M Zca
```

Only needs a RISC-V GCC (and `qemu-system-riscv32` for `--qemu`). It builds
the checked-in unprivileged tests (I, M, Zmmul, Zaamo, Zalrsc, Zca, Zicsr,
Zifencei, Zihintpause, Zihintntl: 104 tests) with a hand-written
`local/rvtest_config.h` instead of the UDB-generated one, takes the expected
values from **MRVE32's own** signature (`mrve32 --htif --signature`), and runs
the self-checking ELFs on MRVE32 and on QEMU's `virt` machine (which has the
same finisher, UART and RAM addresses).

A pass therefore means MRVE32 and QEMU agree at every signature point; it is
a differential test, not a certification result. Zicntr is not included:
its signature build reads `time` natively (Sail implements it), which MRVE32
does not; the self-checking ELFs emulate `time` from `mtime`, so Zicntr runs
in the Sail flow. CI runs this flow on every push and pull request
(job `differential`, Fedora 44: GCC 16, binutils 2.46, QEMU 10.2).
