#!/usr/bin/env bash
# RISC-V Architectural Certification Tests (riscv-arch-test, ACT4) on MRVE32.
# See tests/arch-test/README.md.
#
#   scripts/arch-test.sh run ELF_DIR
#       Runs self-checking ELFs built by the ACT4 framework (expected values
#       from the Sail reference model) with tests/arch-test/mrve32-rv32imac.
#
#   scripts/arch-test.sh local [--qemu] [EXTENSION...]
#       Without Sail or UDB: builds the unprivileged tests with a RISC-V GCC,
#       takes the expected values from MRVE32's own signature, and runs the
#       self-checking ELFs on MRVE32 and, with --qemu, on qemu-system-riscv32
#       (a differential test against QEMU, not a certification run).
#
# Environment: ARCH_TEST (riscv-arch-test checkout; cloned at the pinned
# revision into target/arch-test/src if unset), CROSS (GCC prefix), MRVE32
# (runner binary; built with cargo if unset), JOBS (parallel jobs).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CFG="$ROOT/tests/arch-test/mrve32-rv32imac"
LOCAL_ENV="$ROOT/tests/arch-test/local"
WORK="$ROOT/target/arch-test"
# riscv-arch-test revision the configuration was written against.
ARCH_TEST_REPO=https://github.com/riscv-non-isa/riscv-arch-test.git
ARCH_TEST_REV=2dc36dd2f3d25383039effa10f1a3930a78a945a
# Unprivileged extensions of the local flow (tests/rv32i/<EXT>). Not Zicntr:
# its signature build reads the time CSR natively (as Sail implements it),
# which MRVE32 does not have; the self-checking ELFs emulate it from mtime,
# so Zicntr runs in the Sail flow only.
DEFAULT_EXTENSIONS=(I M Zmmul Zaamo Zalrsc Zca Zicsr Zifencei Zihintpause Zihintntl)
MAX_STEPS=50000000
JOBS="${JOBS:-$(nproc 2>/dev/null || echo 4)}"

die() { echo "arch-test: $*" >&2; exit 2; }

mrve32_bin() {
    if [[ -z "${MRVE32:-}" ]]; then
        cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml" --bin mrve32
        MRVE32="$ROOT/target/release/mrve32"
    fi
    export MRVE32
}

# A self-checking ELF passes when it exits 0 and reports TEST PASSED.
run_one() {
    local runner=$1 elf=$2 log=$3
    local status=0
    case $runner in
        mrve32) "$MRVE32" --max-steps "$MAX_STEPS" "$elf" >"$log" 2>&1 || status=$? ;;
        qemu) timeout 60 qemu-system-riscv32 -M virt -nographic -bios none -kernel "$elf" \
                  >"$log" 2>&1 </dev/null || status=$? ;;
    esac
    [[ $status -eq 0 ]] && grep -q "RVCP-SUMMARY: TEST PASSED" "$log"
}

summarize() {
    local label=$1 total=$2; shift 2
    local failed=("$@")
    echo "== $label: $((total - ${#failed[@]}))/$total passed"
    if ((${#failed[@]})); then
        printf '   FAIL %s\n' "${failed[@]}"
        return 1
    fi
}

cmd_run() {
    local dir=${1:?usage: arch-test.sh run ELF_DIR}
    [[ -d $dir ]] || die "no directory $dir"
    mrve32_bin
    mapfile -t elfs < <(find "$dir" -name '*.elf' ! -name '*.sig.elf' | sort)
    ((${#elfs[@]})) || die "no ELF files in $dir"
    mkdir -p "$WORK/logs"
    local failed=()
    for elf in "${elfs[@]}"; do
        local name; name=$(basename "$elf" .elf)
        run_one mrve32 "$elf" "$WORK/logs/$name.log" || failed+=("$name (log: $WORK/logs/$name.log)")
    done
    summarize "MRVE32 (Sail-generated expected values)" "${#elfs[@]}" "${failed[@]}"
}

find_cross() {
    if [[ -z "${CROSS:-}" ]]; then
        for p in riscv64-unknown-elf- riscv32-unknown-elf- riscv64-linux-gnu-; do
            if command -v "${p}gcc" >/dev/null; then CROSS=$p; break; fi
        done
    fi
    : "${CROSS:?no RISC-V GCC found; set CROSS=<prefix>}"
    export CROSS
}

arch_test_src() {
    if [[ -z "${ARCH_TEST:-}" ]]; then
        ARCH_TEST="$WORK/src"
        if [[ ! -d $ARCH_TEST/.git ]]; then
            git clone --quiet --filter=blob:none "$ARCH_TEST_REPO" "$ARCH_TEST"
        fi
        git -C "$ARCH_TEST" checkout --quiet "$ARCH_TEST_REV"
    fi
    [[ -d $ARCH_TEST/tests/env ]] || die "ARCH_TEST=$ARCH_TEST is not a riscv-arch-test checkout"
    export ARCH_TEST
}

# Builds one test: <name>.sig.elf -> <name>.sig (MRVE32) -> <name>.results
# -> <name>.elf. Prints the ELF path, or FAIL:<reason>.
build_local() {
    local src=$1 out=$2
    local name; name=$(basename "$src" .S)
    local march; march=$(sed -n 's/^# MARCH: //p' "$src")
    local cc=(
        "${CROSS}gcc" -Wl,--no-warn-rwx-segments "-I$CFG" "-T$CFG/link.ld" -O0 -g
        -mcmodel=medany -nostdlib "-I$ARCH_TEST/tests/env" "-I$LOCAL_ENV"
        -Wl,--defsym=__stack_size=128 -march=rv32i -Xassembler "-march=$march"
        -mabi=ilp32 -DTEST_FLEN=0 "-DTEST_FILE=\"$name.S\""
    )
    # As the framework does for Sail: HTIF halt/console, Sail device addresses.
    "${cc[@]}" -DSIGNATURE -DSAIL_CLINT_BASE_ADDRESS=0x2000000 \
        -DSAIL_SIMPLE_INTERRUPT_GENERATOR_BASE_ADDRESS=0xc000000 \
        -o "$out/$name.sig.elf" "$src" 2>"$out/$name.build.log" \
        || { echo "FAIL:$name: signature build (see $out/$name.build.log)"; return; }
    "$MRVE32" --htif --signature "$out/$name.sig" --max-steps "$MAX_STEPS" \
        "$out/$name.sig.elf" >"$out/$name.sig.log" 2>&1 \
        || { echo "FAIL:$name: signature run (see $out/$name.sig.log)"; return; }
    # The framework's sig_modify.process_signature_file.
    awk 'NF { l[n++] = $0 }
         END {
             for (i = 0; i < n; i++) {
                 if (i == n - 1) print "sig_end_canary:"
                 print ".word 0x" l[i]
                 if (l[i] ~ /4b8e2d17/) print "final_sig_offset:"
                 if (l[i] ~ /7a110ff5/) print "final_trap_sig_offset:"
                 if (l[i] ~ /d3a91f6c/) print "trap_sigptr:"
             }
         }' "$out/$name.sig" >"$out/$name.results"
    "${cc[@]}" -DRVTEST_SELFCHECK "-DSIGNATURE_FILE=\"$out/$name.results\"" -DXLEN=32 \
        -o "$out/$name.elf" "$src" 2>>"$out/$name.build.log" \
        || { echo "FAIL:$name: self-checking build (see $out/$name.build.log)"; return; }
    echo "$out/$name.elf"
}

cmd_local() {
    local qemu=0 exts=()
    for arg in "$@"; do
        case $arg in
            --qemu) qemu=1 ;;
            -*) die "unknown option $arg" ;;
            *) exts+=("$arg") ;;
        esac
    done
    ((${#exts[@]})) || exts=("${DEFAULT_EXTENSIONS[@]}")
    if ((qemu)); then command -v qemu-system-riscv32 >/dev/null || die "--qemu: qemu-system-riscv32 not found"; fi
    find_cross
    arch_test_src
    mrve32_bin

    local out="$WORK/local"
    rm -rf "$out"; mkdir -p "$out"
    local srcs=()
    for ext in "${exts[@]}"; do
        [[ -d $ARCH_TEST/tests/rv32i/$ext ]] || die "no tests for $ext in $ARCH_TEST/tests/rv32i"
        srcs+=("$ARCH_TEST"/tests/rv32i/"$ext"/*.S)
    done
    export -f build_local
    export CFG LOCAL_ENV MAX_STEPS
    echo "== building ${#srcs[@]} tests (${exts[*]}) with ${CROSS}gcc, expected values from MRVE32"
    mapfile -t built < <(printf '%s\n' "${srcs[@]}" \
        | xargs -P "$JOBS" -I{} bash -c 'build_local "$1" "$2"' _ {} "$out" | sort)

    local elfs=() failed=()
    for line in "${built[@]}"; do
        if [[ $line == FAIL:* ]]; then failed+=("${line#FAIL:}"); else elfs+=("$line"); fi
    done
    local rc=0
    summarize "build + signature on MRVE32" "${#srcs[@]}" "${failed[@]}" || rc=1

    for runner in mrve32 $( ((qemu)) && echo qemu ); do
        failed=()
        for elf in "${elfs[@]}"; do
            local name; name=$(basename "$elf" .elf)
            run_one "$runner" "$elf" "$out/$name.$runner.log" \
                || failed+=("$name (log: $out/$name.$runner.log)")
        done
        summarize "self-check on $runner" "${#elfs[@]}" "${failed[@]}" || rc=1
    done
    return $rc
}

case "${1:-}" in
    run) shift; cmd_run "$@" ;;
    local) shift; cmd_local "$@" ;;
    *) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
