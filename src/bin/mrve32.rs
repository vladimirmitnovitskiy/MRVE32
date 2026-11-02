//! `mrve32`: runs a bare-metal RV32IMAC ELF on a QEMU-virt-like machine.

use std::cell::Cell;
use std::io::{self, Write};
use std::process::ExitCode;
use std::rc::Rc;

use mrve32::devices::clint::CLINT_SIZE;
use mrve32::devices::finisher::FINISHER_SIZE;
use mrve32::{
    Bus, Clint, Cpu, ElfImage, Exception, Machine, Ram, Reg, StopReason, TestFinisher, Trap,
    TrapCause, Uart, disassemble, is_compressed,
};

const USAGE: &str = "\
Usage: mrve32 [OPTIONS] <FIRMWARE.elf>

Runs bare-metal RV32IMAC firmware on a QEMU-virt-like memory map:
  RAM       0x80000000  (--ram-size, default 128M)
  UART      0x10000000  NS16550A; transmitted bytes go to stdout
  finisher  0x00100000  sifive,test0; firmware writes its exit status here
  CLINT     0x02000000  mtime/mtimecmp/msip

Options:
  --isa rv32imac|rv32ima instruction set; rv32ima (no C) traps on
                         2-byte-aligned jump targets [default: rv32imac]
  --ram-base ADDR        RAM base address [default: 0x80000000]
  --ram-size SIZE        RAM size, optional K/M suffix [default: 128M]
  --uart ADDR|none       UART base address [default: 0x10000000]
  --finisher ADDR|none   test finisher base address [default: 0x100000]
  --clint ADDR|none      CLINT base address [default: 0x2000000]
  --input FILE|-         bytes the firmware receives on the UART (read
                         up front; - reads stdin to EOF)
  --max-steps N          stop after N steps
  --stop-on-trap         stop at the first trap (exception or interrupt)
  --trace                print every executed instruction to stderr
  --stats                print step and instruction counts to stderr at exit
  -h, --help             print this help
  -V, --version          print the version

Exit status:
  0..=255  the firmware's finisher status (codes above 255 become 255)
  123      the hart waits in wfi with no interrupt enabled (never wakes)
  124      --max-steps reached
  125      usage or load error
  126      a trap with --stop-on-trap, or a double fault (the first
           instruction of the trap handler faulted, e.g. mtvec not set)";

const EXIT_IDLE: u8 = 123;
const EXIT_STEP_LIMIT: u8 = 124;
const EXIT_USAGE: u8 = 125;
const EXIT_TRAP: u8 = 126;

struct Options {
    firmware: String,
    compressed: bool,
    ram_base: Reg,
    ram_size: usize,
    uart: Option<Reg>,
    finisher: Option<Reg>,
    clint: Option<Reg>,
    input: Option<String>,
    max_steps: u64,
    stop_on_trap: bool,
    trace: bool,
    stats: bool,
}

enum Parsed {
    Run(Options),
    Exit(ExitCode),
}

fn parse_u64(s: &str) -> Option<u64> {
    let s = s.replace('_', "");
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn parse_addr(opt: &str, s: &str) -> Result<Reg, String> {
    parse_u64(s)
        .and_then(|v| Reg::try_from(v).ok())
        .ok_or_else(|| format!("{opt}: invalid address '{s}'"))
}

fn parse_opt_addr(opt: &str, s: &str) -> Result<Option<Reg>, String> {
    if s == "none" {
        Ok(None)
    } else {
        parse_addr(opt, s).map(Some)
    }
}

fn parse_size(s: &str) -> Result<usize, String> {
    let (num, mul) = match s.as_bytes().last() {
        Some(b'K' | b'k') => (&s[..s.len() - 1], 1 << 10),
        Some(b'M' | b'm') => (&s[..s.len() - 1], 1 << 20),
        _ => (s, 1),
    };
    parse_u64(num)
        .and_then(|n| n.checked_mul(mul))
        .filter(|&n| n > 0 && n <= 1 << 32)
        .map(|n| n as usize)
        .ok_or_else(|| format!("--ram-size: invalid size '{s}'"))
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Parsed, String> {
    let mut opts = Options {
        firmware: String::new(),
        compressed: true,
        ram_base: 0x8000_0000,
        ram_size: 128 << 20,
        uart: Some(0x1000_0000),
        finisher: Some(0x0010_0000),
        clint: Some(0x0200_0000),
        input: None,
        max_steps: u64::MAX,
        stop_on_trap: false,
        trace: false,
        stats: false,
    };
    let mut firmware = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(Parsed::Exit(ExitCode::SUCCESS));
            }
            "-V" | "--version" => {
                println!("mrve32 {}", env!("CARGO_PKG_VERSION"));
                return Ok(Parsed::Exit(ExitCode::SUCCESS));
            }
            "--isa" => {
                let v = value(&arg)?;
                opts.compressed = match v.to_ascii_lowercase().as_str() {
                    "rv32imac" => true,
                    "rv32ima" => false,
                    _ => {
                        return Err(format!(
                            "--isa: unsupported ISA '{v}' (rv32imac or rv32ima)"
                        ));
                    }
                };
            }
            "--ram-base" => opts.ram_base = parse_addr(&arg, &value(&arg)?)?,
            "--ram-size" => opts.ram_size = parse_size(&value(&arg)?)?,
            "--uart" => opts.uart = parse_opt_addr(&arg, &value(&arg)?)?,
            "--finisher" => opts.finisher = parse_opt_addr(&arg, &value(&arg)?)?,
            "--clint" => opts.clint = parse_opt_addr(&arg, &value(&arg)?)?,
            "--input" => opts.input = Some(value(&arg)?),
            "--max-steps" => {
                let v = value(&arg)?;
                opts.max_steps =
                    parse_u64(&v).ok_or_else(|| format!("--max-steps: invalid number '{v}'"))?;
            }
            "--stop-on-trap" => opts.stop_on_trap = true,
            "--trace" => opts.trace = true,
            "--stats" => opts.stats = true,
            s if s.starts_with('-') => return Err(format!("unknown option '{s}'")),
            _ if firmware.is_some() => return Err(format!("unexpected argument '{arg}'")),
            _ => firmware = Some(arg),
        }
    }
    opts.firmware = firmware.ok_or("missing firmware file")?;
    Ok(Parsed::Run(opts))
}

/// `line_open` is set while the firmware's UART output ends mid-line.
fn build_machine(opts: &Options, line_open: Rc<Cell<bool>>) -> Result<Machine, String> {
    let mut bus = Bus::new();
    let mut map = |name: &str, base: Reg, size: Reg, dev: Box<dyn mrve32::BusDevice>| {
        bus.map_device(base, size, dev)
            .map_err(|e| format!("cannot map {name} at 0x{base:08X}: {e}"))
    };
    let ram_len = Reg::try_from(opts.ram_size).unwrap_or(Reg::MAX);
    map(
        "RAM",
        opts.ram_base,
        ram_len,
        Box::new(Ram::new(opts.ram_size)),
    )?;
    let input = match opts.input.as_deref() {
        None => Vec::new(),
        Some("-") => {
            let mut buf = Vec::new();
            io::Read::read_to_end(&mut io::stdin(), &mut buf)
                .map_err(|e| format!("--input: stdin: {e}"))?;
            buf
        }
        Some(path) => std::fs::read(path).map_err(|e| format!("--input: {path}: {e}"))?,
    };
    if opts.uart.is_none() && !input.is_empty() {
        return Err("--input needs a UART".into());
    }
    if let Some(base) = opts.uart {
        let uart = Uart::new();
        uart.io().push_input(&input);
        let mut stdout = io::stdout();
        uart.io().set_sink(Some(Box::new(move |b| {
            // A closed stdout must not stop the firmware.
            let _ = stdout.write_all(&[b]);
            if b == b'\n' {
                let _ = stdout.flush();
            }
            line_open.set(b != b'\n');
        })));
        map("UART", base, uart.size(), Box::new(uart))?;
    }
    if let Some(base) = opts.finisher {
        map(
            "finisher",
            base,
            FINISHER_SIZE,
            Box::new(TestFinisher::new()),
        )?;
    }
    if let Some(base) = opts.clint {
        map("CLINT", base, CLINT_SIZE, Box::new(Clint::new()))?;
    }

    let data = std::fs::read(&opts.firmware).map_err(|e| format!("{}: {e}", opts.firmware))?;
    let image = ElfImage::parse(&data).map_err(|e| format!("{}: {e}", opts.firmware))?;
    let mut cpu = Cpu::new(image.entry);
    cpu.set_compressed(opts.compressed);
    let mut machine = Machine::new(cpu, bus);
    machine
        .load_elf(&image)
        .map_err(|e| format!("{}: {e}", opts.firmware))?;
    Ok(machine)
}

fn describe(trap: &Trap) -> String {
    format!(
        "{} (mcause=0x{:08x}, mtval=0x{:08x}, mepc=0x{:08x})",
        trap.cause,
        trap.cause.mcause(),
        trap.cause.tval(),
        trap.epc
    )
}

fn main() -> ExitCode {
    let opts = match parse_args(std::env::args().skip(1)) {
        Ok(Parsed::Run(opts)) => opts,
        Ok(Parsed::Exit(code)) => return code,
        Err(msg) => {
            eprintln!("mrve32: {msg}\n\n{USAGE}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let line_open: Rc<Cell<bool>> = Rc::default();
    let mut machine = match build_machine(&opts, line_open.clone()) {
        Ok(m) => m,
        Err(msg) => {
            eprintln!("mrve32: {msg}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    // Double-fault detection: a step that traps right after a step that
    // trapped means the handler's first instruction faulted.
    let double_fault: Rc<Cell<Option<(Trap, Trap)>>> = Rc::default();
    let last_trap: Cell<Option<Trap>> = Cell::new(None);
    let detected = double_fault.clone();
    let trace = opts.trace;
    machine.set_tracer(Some(Box::new(move |e| {
        if trace {
            let mut err = io::stderr().lock();
            if let Some(raw) = e.instruction {
                let text = disassemble(raw, Some(e.pc));
                let _ = if is_compressed(raw) {
                    writeln!(err, "{:08x}: {raw:04x}      {text}", e.pc)
                } else {
                    writeln!(err, "{:08x}: {raw:08x}  {text}", e.pc)
                };
            }
            if let Some(trap) = &e.trap {
                let _ = writeln!(err, "          trap: {}", describe(trap));
            }
        }
        if let (Some(prev), Some(trap)) = (last_trap.get(), e.trap)
            && matches!(trap.cause, TrapCause::Exception(_))
        {
            detected.set(Some((prev, trap)));
        }
        last_trap.set(e.trap);
    })));

    let res = machine.run_until(opts.max_steps, opts.stop_on_trap, || {
        double_fault.get().is_some()
    });
    let _ = io::stdout().flush();
    let exit_requested = matches!(res.reason, StopReason::Exit(_));
    if line_open.get() && (!exit_requested || opts.stats) {
        // Start runner messages on a fresh line.
        eprintln!();
    }

    let code = match res.reason {
        StopReason::Exit(code) => u8::try_from(code).unwrap_or(255),
        StopReason::Trap(trap) => {
            eprintln!("mrve32: trap: {}", describe(&trap));
            EXIT_TRAP
        }
        StopReason::Aborted => {
            let (first, second) = double_fault.get().expect("abort only on double fault");
            eprintln!("mrve32: trap: {}", describe(&first));
            let hint = match second.cause {
                TrapCause::Exception(Exception::InstructionAccessFault(_)) => " (is mtvec set?)",
                _ => "",
            };
            eprintln!(
                "mrve32: double fault in the trap handler{hint}: {}",
                describe(&second)
            );
            EXIT_TRAP
        }
        StopReason::Idle => {
            eprintln!(
                "mrve32: hart idle at 0x{:08x}: wfi with no interrupt enabled",
                machine.cpu.pc
            );
            EXIT_IDLE
        }
        StopReason::MaxSteps => {
            eprintln!(
                "mrve32: step limit reached ({} steps) at 0x{:08x}",
                res.steps, machine.cpu.pc
            );
            EXIT_STEP_LIMIT
        }
        StopReason::Breakpoint(_) => unreachable!("the runner sets no breakpoints"),
    };
    if opts.stats {
        eprintln!(
            "mrve32: {} steps, {} instructions retired, mcycle {}",
            res.steps, machine.cpu.csrs.minstret, machine.cpu.csrs.mcycle
        );
    }
    ExitCode::from(code)
}
