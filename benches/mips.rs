//! Interpreter throughput in millions of instructions per second (MIPS).
//!
//! `cargo bench --bench mips` (std only, no external harness). Each case
//! runs a tight loop for a fixed number of steps; the best of several runs
//! is reported. Set `MRVE32_BENCH_STEPS` to change the step count.

use std::hint::black_box;
use std::time::{Duration, Instant};

use mrve32::{Bus, BusDevice, Clint, Cpu, Machine, Ram, Reg, TraceEvent};

const BASE: Reg = 0x8000_0000;
const RUNS: usize = 5;

/// add/xor/slli/sub/mul/srai/or; j
const ALU: &[u32] = &[
    0x00b50533, 0x00a5c5b3, 0x00351613, 0x40b606b3, 0x02a68733, 0x40775793, 0x00f5e5b3, 0xfe5ff06f,
];
/// lui s0, 0x80001; loop: lw/addi/sw/lw/sb/lhu/sw; j loop
const MEMORY: &[u32] = &[
    0x80001437, 0x00042283, 0x00128293, 0x00542223, 0x00842303, 0x006400a3, 0x00245383, 0x00742023,
    0xfe5ff06f,
];
/// li t0, 100; loop: addi t0, t0, -1; bnez t0, loop; j start
const BRANCH: &[u32] = &[0x06400293, 0xfff28293, 0xfe029ee3, 0xff5ff06f];
/// c.add/c.xor/c.slli/c.sub/c.srai/c.or/c.addi; c.j (as 16-bit pairs)
const COMPRESSED: &[u32] = &[0x8da9_952e, 0x8d89_050e, 0x8d4d_859d, 0xbfcd_0505];

fn machine(program: &[u32]) -> Machine {
    let mut bus = Bus::new();
    bus.map_device(BASE, 64 * 1024, Box::new(Ram::new(64 * 1024)))
        .unwrap();
    bus.map_device(0x0200_0000, 0x10000, Box::new(Clint::new()))
        .unwrap();
    for (i, &w) in program.iter().enumerate() {
        bus.write_u32(BASE + 4 * i as Reg, w).unwrap();
    }
    Machine::new(Cpu::new(BASE), bus)
}

fn bench(name: &str, steps: u64, setup: impl Fn() -> Machine) {
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let mut m = setup();
        let start = Instant::now();
        let res = m.run(black_box(steps), true);
        let elapsed = start.elapsed();
        assert_eq!(res.steps, steps, "{name}: stopped early: {:?}", res.reason);
        best = best.min(elapsed);
    }
    let mips = steps as f64 / best.as_secs_f64() / 1e6;
    println!("{name:<24} {mips:>8.1} MIPS  ({:.1} ns/instr)", 1e3 / mips);
}

fn main() {
    // `cargo test --benches` runs this with `--bench` absent: keep it short.
    let quick = !std::env::args().any(|a| a == "--bench");
    let steps = std::env::var("MRVE32_BENCH_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if quick { 10_000 } else { 20_000_000 });

    println!("{steps} steps per run, best of {RUNS}");
    bench("alu (rv32im)", steps, || machine(ALU));
    bench("memory (lw/sw/sb/lhu)", steps, || machine(MEMORY));
    bench("branch loop", steps, || machine(BRANCH));
    bench("compressed (rv32c)", steps, || machine(COMPRESSED));
    bench("alu + tracer", steps, || {
        let mut m = machine(ALU);
        m.set_tracer(Some(Box::new(|e: &TraceEvent| {
            black_box(e.pc);
        })));
        m
    });
}
