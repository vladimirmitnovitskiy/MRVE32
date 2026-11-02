//! End-to-end tests of the `mrve32` runner on C firmware built with a
//! standard RISC-V GCC (`tests/firmware`, `scripts/build-test-firmware.sh`).

use std::io::Write;
use std::process::{Command, Output, Stdio};

const FIRMWARE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/firmware/hello.elf");
const HTIF_FIRMWARE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/firmware/htif.elf");
const GREETING: &str = "Hello from MRVE32!\nfib(20) = 6765\n";

fn run(args: &[&str], input: Option<&[u8]>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mrve32"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("runner starts");
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input.unwrap_or_default()).unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn test_firmware_prints_and_passes() {
    let out = run(&[FIRMWARE], None);
    assert_eq!(stdout(&out), GREETING);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert_eq!(stderr(&out), "");
}

#[test]
fn test_finisher_status_and_uart_input() {
    let out = run(&["--input", "-", FIRMWARE], Some(b"fail"));
    assert_eq!(stdout(&out), format!("{GREETING}input: fail"));
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn test_double_fault_without_trap_handler() {
    let out = run(&["--input", "-", FIRMWARE], Some(b"c"));
    assert_eq!(out.status.code(), Some(126));
    let err = stderr(&out);
    assert!(
        err.starts_with("\nmrve32: trap: illegal instruction"),
        "{err}"
    );
    assert!(
        err.contains("double fault in the trap handler (is mtvec set?)"),
        "{err}"
    );
}

#[test]
fn test_idle_and_step_limit() {
    let out = run(&["--input", "-", FIRMWARE], Some(b"w"));
    assert_eq!(out.status.code(), Some(123));
    assert!(stderr(&out).contains("wfi with no interrupt enabled"));

    let out = run(&["--max-steps", "50", FIRMWARE], None);
    assert_eq!(out.status.code(), Some(124));
    assert!(stderr(&out).contains("step limit reached (50 steps)"));
}

#[test]
fn test_trace_and_stats() {
    let out = run(&["--trace", "--stats", FIRMWARE], None);
    assert_eq!(out.status.code(), Some(0));
    let err = stderr(&out);
    let first = err.lines().next().unwrap();
    assert_eq!(first, "80000000: 00100117  auipc sp, 0x100");
    assert!(
        err.contains("80000024: 2011      jal 0x80000028"),
        "c.jal main"
    );
    assert!(err.lines().last().unwrap().contains("instructions retired"));
}

#[test]
fn test_rv32ima_rejects_compressed_firmware() {
    // The firmware is built for rv32imac; without C its first 16-bit
    // instruction is part of an illegal 32-bit word.
    let out = run(&["--isa", "rv32ima", FIRMWARE], None);
    assert_eq!(out.status.code(), Some(126));
    assert!(
        stderr(&out).contains("illegal instruction"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn test_stop_on_trap() {
    let out = run(&["--stop-on-trap", "--uart", "none", FIRMWARE], None);
    assert_eq!(out.status.code(), Some(126));
    assert!(
        stderr(&out).contains("load access fault: 0x10000005"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn test_usage_errors() {
    for args in [
        &["--bogus"][..],
        &["--isa", "rv64gc", FIRMWARE],
        &[],
        &["--ram-size", "0", FIRMWARE],
        &["--uart", "0x80000000", FIRMWARE],
        &["/nonexistent.elf"],
    ] {
        let out = run(args, None);
        assert_eq!(out.status.code(), Some(125), "{args:?}: {}", stderr(&out));
        assert!(stderr(&out).starts_with("mrve32: "), "{args:?}");
    }
    let out = run(&["--version"], None);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).starts_with("mrve32 "));
}

#[test]
fn test_htif_console_exit_and_signature() {
    let dir = std::env::temp_dir().join(format!("mrve32-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sig = dir.join("htif.sig");
    let out = run(
        &[
            "--htif",
            "--signature",
            sig.to_str().unwrap(),
            HTIF_FIRMWARE,
        ],
        None,
    );
    assert_eq!(stdout(&out), "hi\n");
    assert_eq!(out.status.code(), Some(3), "stderr: {}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(&sig).unwrap(),
        "cafef00d\n12345678\ndeadbeef\n"
    );
    std::fs::remove_dir_all(&dir).unwrap();

    // Without --htif, tohost is plain memory and the firmware spins.
    let out = run(&["--max-steps", "1000", HTIF_FIRMWARE], None);
    assert_eq!(out.status.code(), Some(124));
    assert_eq!(stdout(&out), "");
}

#[test]
fn test_htif_and_signature_need_symbols() {
    let out = run(&["--htif", FIRMWARE], None);
    assert_eq!(out.status.code(), Some(125));
    assert!(stderr(&out).contains("--htif: the ELF has no symbol `tohost`"));
    let out = run(&["--signature", "x.sig", FIRMWARE], None);
    assert_eq!(out.status.code(), Some(125));
    assert!(stderr(&out).contains("no symbol `begin_signature`"));
}
