[![Rust](https://img.shields.io/badge/rust-stable%202024-orange.svg)](https://www.rust-lang.org)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

# MRVE32 (Modular RISC-V Emulator 32-bit)

A modular, bare-metal RV32I emulator implemented from scratch in pure Rust (`std`-only).

### Key Features

- **Modular Bus Architecture**: Pluggable memory-mapped devices via the `BusDevice` trait.
- **Coprocessor Slot**: Extension point (`Coprocessor` trait) for custom RISC-V opcodes (`0x0B`, `0x2B`).
- **Idiomatic Rust Ownership**: Unidirectional execution model (`cpu.step(&mut bus)`).
- **Extensible**: Designed as an embeddable library for research, custom accelerators, and educational purposes.

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## License

This project is licensed under the Apache License, Version 2.0. See the LICENSE file for details.
