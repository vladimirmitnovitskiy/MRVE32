#!/usr/bin/env bash
# Builds tests/firmware/hello.elf (used by tests/cli.rs) with a RISC-V GCC.
# Reproducible: fixed object names, no build id, source paths remapped.
#
#   CROSS=riscv64-unknown-elf- scripts/build-test-firmware.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/tests/firmware"

if [[ -z "${CROSS:-}" ]]; then
    for p in riscv64-unknown-elf- riscv32-unknown-elf- riscv64-linux-gnu-; do
        if command -v "${p}gcc" >/dev/null; then CROSS=$p; break; fi
    done
fi
: "${CROSS:?no RISC-V GCC found; set CROSS=<prefix>}"

CFLAGS=(-march=rv32imc_zicsr -mabi=ilp32 -O2 -ffreestanding -fno-pic
        -fno-asynchronous-unwind-tables -Wall -Wextra -Werror
        "-ffile-prefix-map=$SRC=.")

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$SRC"
"${CROSS}gcc" "${CFLAGS[@]}" -c start.S -o "$WORK/start.o"
"${CROSS}gcc" "${CFLAGS[@]}" -c hello.c -o "$WORK/hello.o"
"${CROSS}gcc" -march=rv32imc_zicsr -mabi=ilp32 -nostdlib -nostartfiles -static \
    -Wl,--build-id=none -Wl,--no-warn-rwx-segments -T link.ld "$WORK/start.o" "$WORK/hello.o" -o hello.elf
echo "built $SRC/hello.elf with ${CROSS}gcc"
