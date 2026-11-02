#!/usr/bin/env bash
# Rebuilds the riscv-tests ELF files in tests/isa/ from source.
#
# Usage: scripts/build-riscv-tests.sh [path/to/riscv-tests]
#
# Requires a RISC-V GCC cross toolchain (riscv64-unknown-elf-* or
# riscv64-linux-gnu-*). Override with CROSS=<prefix>.
# If no riscv-tests checkout is given, it is cloned at $RISCV_TESTS_REV.
set -euo pipefail

RISCV_TESTS_REV="${RISCV_TESTS_REV:-3df703cd52d5f0d8224e3c50bf03c6cb70efff98}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${OUT:-$ROOT/tests/isa}"

if [[ -z "${CROSS:-}" ]]; then
    for p in riscv64-unknown-elf- riscv32-unknown-elf- riscv64-linux-gnu-; do
        if command -v "${p}gcc" >/dev/null; then CROSS="$p"; break; fi
    done
fi
: "${CROSS:?no RISC-V cross toolchain found, set CROSS=<prefix>}"

SRC="${1:-}"
if [[ -z "$SRC" ]]; then
    SRC="$(mktemp -d)/riscv-tests"
    git clone --quiet --recursive https://github.com/riscv-software-src/riscv-tests.git "$SRC"
    git -C "$SRC" checkout --quiet "$RISCV_TESTS_REV"
    git -C "$SRC" submodule update --quiet --init --recursive
fi
ISA="$SRC/isa"

# Only the tests MRVE32 targets: M-mode-only RV32IMC + Zicsr, no A/F/S/U.
# rv32mi tests that need PMP (pmpaddr) or Sdtrig (breakpoint) are not included.
UI_TESTS="add addi and andi auipc beq bge bgeu blt bltu bne fence_i jal jalr
          lb lbu ld_st lh lhu lui lw ma_data or ori sb sh simple sll slli slt
          slti sltiu sltu sra srai srl srli st_ld sub sw xor xori"
UM_TESTS="div divu mul mulh mulhsu mulhu rem remu"
MI_TESTS="csr illegal ma_addr ma_fetch mcsr sbreak scall shamt
          lh-misaligned lw-misaligned sh-misaligned sw-misaligned
          zicntr instret_overflow"
UC_TESTS="rvc"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Assemble and link in separate steps with a fixed object name: a one-step
# gcc build records a random temp object name (ccXXXXXX.o) in the symbol
# table, which makes every rebuild differ.
build() { # <suite dir> <test name> <output name> [march]
    local flags=(-march="${4:-rv32im_zicsr_zifencei}" -mabi=ilp32 -mcmodel=medany
                 -fvisibility=hidden)
    (
        cd "$WORK"
        "${CROSS}gcc" "${flags[@]}" -I"$SRC/env/p" -I"$ISA/macros/scalar" \
            -c "$ISA/$1/$2.S" -o "$3.o"
        "${CROSS}gcc" "${flags[@]}" -static -nostdlib -nostartfiles \
            -T"$SRC/env/p/link.ld" "$3.o" -o "$OUT/$3.elf"
    )
}

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
for t in $UI_TESTS; do build rv32ui "$t" "rv32ui-p-$t"; done
for t in $UM_TESTS; do build rv32um "$t" "rv32um-p-$t"; done
for t in $MI_TESTS; do build rv32mi "$t" "rv32mi-p-$t"; done
for t in $UC_TESTS; do build rv32uc "$t" "rv32uc-p-$t" rv32imc_zicsr_zifencei; done
echo "built $(ls "$OUT"/*.elf | wc -l) ELF files into $OUT"
