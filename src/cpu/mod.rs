use crate::bus::{BusDevice, BusError};
use crate::cpu::coprocessor::Coprocessor;
use crate::cpu::instructions::Instruction;
use crate::types::Reg;

pub mod coprocessor;
pub mod decode;
pub mod instructions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    IllegalInstruction(u32),
    Bus(BusError),
}

pub struct Cpu {
    regs: [Reg; 32],
    pub pc: Reg,
    coprocessor: Option<Box<dyn Coprocessor>>,
}

impl From<BusError> for CpuError {
    fn from(err: BusError) -> Self {
        CpuError::Bus(err)
    }
}

impl Cpu {
    pub fn new(pc: Reg) -> Self {
        Self {
            regs: [0; 32],
            pc,
            coprocessor: None,
        }
    }
    pub fn read_reg(&self, reg: usize) -> Reg {
        if reg == 0 { 0 } else { self.regs[reg] }
    }
    pub fn write_reg(&mut self, reg: usize, val: Reg) {
        if reg != 0 {
            self.regs[reg] = val;
        }
    }
    pub fn attach_coprocessor(&mut self, coprocessor: Box<dyn Coprocessor>) {
        self.coprocessor = Some(coprocessor);
    }

    pub fn step(&mut self, bus: &mut dyn BusDevice) -> Result<(), CpuError> {
        // 1. Fetch
        let raw = bus.read_u32(self.pc)?;
        // 2. Decode
        let inst = decode::decode(raw)?;

        let mut next_pc = self.pc.wrapping_add(4);
        // 3. Execute
        match inst {
            // R-type
            Instruction::Add { rd, rs1, rs2 } => {
                let res = self.read_reg(rs1).wrapping_add(self.read_reg(rs2));
                self.write_reg(rd, res);
            }
            Instruction::Sub { rd, rs1, rs2 } => {
                let res = self.read_reg(rs1).wrapping_sub(self.read_reg(rs2));
                self.write_reg(rd, res);
            }
            Instruction::Xor { rd, rs1, rs2 } => {
                let res = self.read_reg(rs1) ^ self.read_reg(rs2);
                self.write_reg(rd, res);
            }
            Instruction::Or { rd, rs1, rs2 } => {
                let res = self.read_reg(rs1) | self.read_reg(rs2);
                self.write_reg(rd, res);
            }
            Instruction::And { rd, rs1, rs2 } => {
                let res = self.read_reg(rs1) & self.read_reg(rs2);
                self.write_reg(rd, res);
            }
            Instruction::Sll { rd, rs1, rs2 } => {
                let shamt = self.read_reg(rs2) & 0x1F;
                self.write_reg(rd, self.read_reg(rs1) << shamt);
            }
            Instruction::Srl { rd, rs1, rs2 } => {
                let shamt = self.read_reg(rs2) & 0x1F;
                self.write_reg(rd, self.read_reg(rs1) >> shamt);
            }
            Instruction::Sra { rd, rs1, rs2 } => {
                let shamt = self.read_reg(rs2) & 0x1F;
                let res = ((self.read_reg(rs1) as i32) >> shamt) as u32;
                self.write_reg(rd, res);
            }
            Instruction::Slt { rd, rs1, rs2 } => {
                let res = if (self.read_reg(rs1) as i32) < (self.read_reg(rs2) as i32) {
                    1
                } else {
                    0
                };
                self.write_reg(rd, res);
            }
            Instruction::Sltu { rd, rs1, rs2 } => {
                let res = if self.read_reg(rs1) < self.read_reg(rs2) {
                    1
                } else {
                    0
                };
                self.write_reg(rd, res);
            }

            // I-type
            Instruction::Addi { rd, rs1, imm } => {
                let res = self.read_reg(rs1).wrapping_add(imm as u32);
                self.write_reg(rd, res);
            }
            Instruction::Xori { rd, rs1, imm } => {
                let res = self.read_reg(rs1) ^ (imm as u32);
                self.write_reg(rd, res);
            }
            Instruction::Ori { rd, rs1, imm } => {
                let res = self.read_reg(rs1) | (imm as u32);
                self.write_reg(rd, res);
            }
            Instruction::Andi { rd, rs1, imm } => {
                self.write_reg(rd, self.read_reg(rs1) & (imm as u32));
            }
            Instruction::Slli { rd, rs1, imm } => {
                let shamt = (imm as u32) & 0x1F;
                self.write_reg(rd, self.read_reg(rs1) << shamt);
            }
            Instruction::Srli { rd, rs1, imm } => {
                let shamt = (imm as u32) & 0x1F;
                self.write_reg(rd, self.read_reg(rs1) >> shamt);
            }
            Instruction::Srai { rd, rs1, imm } => {
                let shamt = (imm as u32) & 0x1F;
                let res = ((self.read_reg(rs1) as i32) >> shamt) as u32;
                self.write_reg(rd, res);
            }
            Instruction::Slti { rd, rs1, imm } => {
                let res = if (self.read_reg(rs1) as i32) < imm {
                    1
                } else {
                    0
                };
                self.write_reg(rd, res);
            }
            Instruction::Sltiu { rd, rs1, imm } => {
                let res = if self.read_reg(rs1) < (imm as u32) {
                    1
                } else {
                    0
                };
                self.write_reg(rd, res);
            }

            // U-type
            Instruction::Lui { rd, imm } => {
                self.write_reg(rd, imm as u32);
            }
            Instruction::Auipc { rd, imm } => {
                let res = self.pc.wrapping_add(imm as u32);
                self.write_reg(rd, res);
            }

            // J-type
            Instruction::Jal { rd, imm } => {
                self.write_reg(rd, next_pc);
                next_pc = self.pc.wrapping_add(imm as u32);
            }
            Instruction::Jalr { rd, rs1, imm } => {
                let target = (self.read_reg(rs1).wrapping_add(imm as u32)) & !1;
                self.write_reg(rd, next_pc);
                next_pc = target;
            }

            // Conditional Brunches
            Instruction::Beq { rs1, rs2, imm } => {
                if self.read_reg(rs1) == self.read_reg(rs2) {
                    next_pc = self.pc.wrapping_add(imm as u32);
                }
            }
            Instruction::Bne { rs1, rs2, imm } => {
                if self.read_reg(rs1) != self.read_reg(rs2) {
                    next_pc = self.pc.wrapping_add(imm as u32);
                }
            }
            Instruction::Blt { rs1, rs2, imm } => {
                if (self.read_reg(rs1) as i32) < (self.read_reg(rs2) as i32) {
                    next_pc = self.pc.wrapping_add(imm as u32);
                }
            }
            Instruction::Bge { rs1, rs2, imm } => {
                if (self.read_reg(rs1) as i32) >= (self.read_reg(rs2) as i32) {
                    next_pc = self.pc.wrapping_add(imm as u32);
                }
            }
            Instruction::Bltu { rs1, rs2, imm } => {
                if self.read_reg(rs1) < self.read_reg(rs2) {
                    next_pc = self.pc.wrapping_add(imm as u32);
                }
            }
            Instruction::Bgeu { rs1, rs2, imm } => {
                if self.read_reg(rs1) >= self.read_reg(rs2) {
                    next_pc = self.pc.wrapping_add(imm as u32);
                }
            }

            // Loads
            Instruction::Lw { rd, rs1, imm } => {
                let addr = self.read_reg(rs1).wrapping_add(imm as u32);
                let val = bus.read_u32(addr)?;
                self.write_reg(rd, val);
            }
            Instruction::Lh { rd, rs1, imm } => {
                let addr = self.read_reg(rs1).wrapping_add(imm as u32);
                let val = bus.read_u16(addr)? as i16 as i32 as u32;
                self.write_reg(rd, val);
            }
            Instruction::Lhu { rd, rs1, imm } => {
                let addr = self.read_reg(rs1).wrapping_add(imm as u32);
                let val = bus.read_u16(addr)? as u32;
                self.write_reg(rd, val);
            }
            Instruction::Lb { rd, rs1, imm } => {
                let addr = self.read_reg(rs1).wrapping_add(imm as u32);
                let val = bus.read_u8(addr)? as i8 as i32 as u32;
                self.write_reg(rd, val);
            }
            Instruction::Lbu { rd, rs1, imm } => {
                let addr = self.read_reg(rs1).wrapping_add(imm as u32);
                let val = bus.read_u8(addr)? as u32;
                self.write_reg(rd, val);
            }

            // Stores
            Instruction::Sw { rs1, rs2, imm } => {
                let addr = self.read_reg(rs1).wrapping_add(imm as u32);
                bus.write_u32(addr, self.read_reg(rs2))?;
            }
            Instruction::Sh { rs1, rs2, imm } => {
                let addr = self.read_reg(rs1).wrapping_add(imm as u32);
                bus.write_u16(addr, self.read_reg(rs2) as u16)?;
            }
            Instruction::Sb { rs1, rs2, imm } => {
                let addr = self.read_reg(rs1).wrapping_add(imm as u32);
                bus.write_u8(addr, self.read_reg(rs2) as u8)?;
            }

            // System commands
            Instruction::Ecall | Instruction::Ebreak => {
                // with bare-metal mode without OS works like NOP
            }

            // Custom
            Instruction::Custom { opcode, raw } => {
                if let Some(ref mut coproc) = self.coprocessor {
                    coproc.execute(opcode, raw, &mut self.regs, &mut next_pc, bus)?;
                    self.regs[0] = 0;
                } else {
                    return Err(CpuError::IllegalInstruction(raw));
                }
            }
        }

        self.pc = next_pc;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock-сопроцессор для проверки механизма attach_coprocessor
    struct MockCoprocessor;

    impl Coprocessor for MockCoprocessor {
        fn execute(
            &mut self,
            _opcode: u8,
            _raw: u32,
            _regs: &mut [Reg; 32],
            _pc: &mut Reg,
            _bus: &mut dyn crate::bus::BusDevice,
        ) -> Result<(), CpuError> {
            Ok(())
        }
    }

    #[test]
    fn test_cpu_initialization() {
        let initial_pc: Reg = 0x8000_0000;
        let cpu = Cpu::new(initial_pc);

        assert_eq!(cpu.pc, initial_pc);
        assert!(cpu.coprocessor.is_none());

        // Все регистры x0-x31 должны быть обнулены при старте
        for reg in 0..32 {
            assert_eq!(
                cpu.read_reg(reg),
                0,
                "Регистр x{reg} должен быть 0 при инициализации"
            );
        }
    }

    #[test]
    fn test_zero_register_invariant() {
        let mut cpu = Cpu::new(0x1000);

        // x0 всегда должен возвращать 0
        assert_eq!(cpu.read_reg(0), 0);

        // Попытка записать ненулевое значение в x0
        cpu.write_reg(0, 0xDEAD_BEEF);
        assert_eq!(
            cpu.read_reg(0),
            0,
            "Регистр x0 (zero) должен быть жестко привязан к 0 и игнорировать запись"
        );
    }

    #[test]
    fn test_general_purpose_registers_read_write() {
        let mut cpu = Cpu::new(0x0);

        // Проверяем запись и чтение для всех регистров от x1 до x31
        for reg in 1..32 {
            let value = (reg as Reg) * 0x0101_0101;
            cpu.write_reg(reg, value);
            assert_eq!(
                cpu.read_reg(reg),
                value,
                "Значение в x{reg} не совпадает с записанным"
            );
        }

        // Убеждаемся, что запись в x1..x31 не повлияла на x0
        assert_eq!(cpu.read_reg(0), 0);
    }

    #[test]
    fn test_overwrite_register() {
        let mut cpu = Cpu::new(0x0);

        cpu.write_reg(5, 42);
        assert_eq!(cpu.read_reg(5), 42);

        cpu.write_reg(5, 999);
        assert_eq!(cpu.read_reg(5), 999);
    }

    #[test]
    fn test_attach_coprocessor() {
        let mut cpu = Cpu::new(0x0);
        assert!(cpu.coprocessor.is_none());

        cpu.attach_coprocessor(Box::new(MockCoprocessor));
        assert!(cpu.coprocessor.is_some());
    }

    #[test]
    fn test_bus_error_conversion() {
        // Предполагается типовое перечисление BusError или его инстанс
        // Тест проверяет реализацию трейта From<BusError> for CpuError
        let bus_err = BusError::AddressNotFound(0x1000); // или BusError::LoadAccessFault, в зависимости от определения в bus.rs
        let cpu_err: CpuError = bus_err.into();

        assert_eq!(cpu_err, CpuError::Bus(bus_err));
    }

    #[test]
    #[should_panic]
    fn test_out_of_bounds_register_read_panics() {
        let cpu = Cpu::new(0x0);
        // Чтение несуществующего индекса регистра должно вызывать panic
        let _ = cpu.read_reg(32);
    }

    #[test]
    #[should_panic]
    fn test_out_of_bounds_register_write_panics() {
        let mut cpu = Cpu::new(0x0);
        // Запись в несуществующий индекс регистра должна вызывать panic
        cpu.write_reg(32, 0x1234);
    }

    #[test]
    fn test_cpu_executes_addi_and_add() {
        use crate::bus::Bus;
        use crate::devices::ram::Ram;

        let mut bus = Bus::new();
        let mut ram = Ram::new(1024);

        // Запишем в память маленькую программу:
        // 1. addi x1, x0, 10  (машинный код: 0x00a00093)
        // 2. add  x2, x1, x1  (машинный код: 0x00108133) -> должно стать 20!
        ram.write_u32(0, 0x00A0_0093).unwrap();
        ram.write_u32(4, 0x0010_8133).unwrap();

        // Подключаем RAM по адресу 0x1000
        bus.map_device(0x1000, 1024, Box::new(ram));

        // Стартуем процессор с адреса 0x1000
        let mut cpu = Cpu::new(0x1000);

        // Такт 1: исполняем addi x1, x0, 10
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(1), 10);
        assert_eq!(cpu.pc, 0x1004);

        // Такт 2: исполняем add x2, x1, x1
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(2), 20);
        assert_eq!(cpu.pc, 0x1008);
    }

    // Вспомогательная функция для быстрой загрузки программы в RAM и создания CPU
    fn setup_env(program: &[u32]) -> (Cpu, crate::bus::Bus) {
        use crate::bus::Bus;
        use crate::devices::ram::Ram;

        let mut bus = Bus::new();
        let mut ram = Ram::new(4096);

        for (i, &inst) in program.iter().enumerate() {
            ram.write_u32((i * 4) as u32, inst).unwrap();
        }

        bus.map_device(0x1000, 4096, Box::new(ram));
        let cpu = Cpu::new(0x1000);
        (cpu, bus)
    }

    #[test]
    fn test_alu_r_type_and_i_type_bitwise() {
        // 1. addi x1, x0, -1    (0xFFF0_0093) -> x1 = 0xFFFF_FFFF
        // 2. xori x2, x1, 0x0F  (0x00F0_C113) -> x2 = 0xFFFF_FFF0
        // 3. andi x3, x2, 0xFF  (0x0FF1_7193) -> x3 = 0x0000_00F0
        // 4. ori  x4, x3, 0x05  (0x0051_E213) -> x4 = 0x0000_00F5
        // 5. sub  x5, x4, x3    (0x4032_02B3) -> x5 = 0xF5 - 0xF0 = 5
        // 6. xor  x6, x4, x3    (0x0032_4333) -> x6 = 5
        // 7. or   x7, x3, x5    (0x0051_E3B3) -> x7 = 0xF5
        // 8. and  x8, x4, x5    (0x0052_7433) -> x8 = 5
        let prog = [
            0xFFF0_0093,
            0x00F0_C113,
            0x0FF1_7193,
            0x0051_E213,
            0x4032_02B3,
            0x0032_4333,
            0x0051_E3B3,
            0x0052_7433,
        ];
        let (mut cpu, mut bus) = setup_env(&prog);

        for _ in 0..prog.len() {
            cpu.step(&mut bus).unwrap();
        }

        assert_eq!(cpu.read_reg(1), 0xFFFF_FFFF);
        assert_eq!(cpu.read_reg(2), 0xFFFF_FFF0);
        assert_eq!(cpu.read_reg(3), 0x0000_00F0);
        assert_eq!(cpu.read_reg(4), 0x0000_00F5);
        assert_eq!(cpu.read_reg(5), 5);
        assert_eq!(cpu.read_reg(6), 5);
        assert_eq!(cpu.read_reg(7), 0x0000_00F5);
        assert_eq!(cpu.read_reg(8), 5);
    }

    #[test]
    fn test_shifts_sll_srl_sra() {
        let (mut cpu, mut bus) = setup_env(&[
            0x0040_9113, // slli x2, x1, 4
            0x0040_D193, // srli x3, x1, 4
            0x4040_D213, // srai x4, x1, 4
            0x0050_9433, // sll  x8, x1, x5
            0x0050_D333, // srl  x6, x1, x5
            0x4050_D3B3, // sra  x7, x1, x5
        ]);

        // Задаем отрицательное число в x1 (старший бит = 1) и величину сдвига в x5
        cpu.write_reg(1, 0xF000_0000);
        cpu.write_reg(5, 8);

        for _ in 0..6 {
            cpu.step(&mut bus).unwrap();
        }

        assert_eq!(cpu.read_reg(2), 0x0000_0000); // 0xF000_0000 << 4
        assert_eq!(cpu.read_reg(3), 0x0F00_0000); // логический сдвиг вправо (нули слева)
        assert_eq!(cpu.read_reg(4), 0xFF00_0000); // арифметический сдвиг (знаковый бит сохраняется)
        assert_eq!(cpu.read_reg(8), 0x0000_0000); // 0xF000_0000 << 8
        assert_eq!(cpu.read_reg(6), 0x00F0_0000); // логический >> 8
        assert_eq!(cpu.read_reg(7), 0xFFF0_0000); // арифметический >> 8
    }

    #[test]
    fn test_comparisons_slt_sltu_slti_sltiu() {
        let (mut cpu, mut bus) = setup_env(&[
            0x0020_A1B3, // slt   x3, x1, x2 (signed: -10 < 10 -> 1)
            0x0020_B233, // sltu  x4, x1, x2 (unsigned: 0xFFFFFFF6 < 10 -> 0)
            0x0000_A293, // slti  x5, x1, 0  (signed: -10 < 0 -> 1)
            0xFFF1_3313, // sltiu x6, x2, -1 (unsigned: 10 < 0xFFFFFFFF -> 1)
        ]);

        cpu.write_reg(1, (-10i32) as u32);
        cpu.write_reg(2, 10);

        for _ in 0..4 {
            cpu.step(&mut bus).unwrap();
        }

        assert_eq!(cpu.read_reg(3), 1);
        assert_eq!(cpu.read_reg(4), 0);
        assert_eq!(cpu.read_reg(5), 1);
        assert_eq!(cpu.read_reg(6), 1);
    }

    #[test]
    fn test_lui_and_auipc() {
        let (mut cpu, mut bus) = setup_env(&[
            0x1234_50B7, // lui   x1, 0x12345 -> x1 = 0x1234_5000
            0x0000_1117, // auipc x2, 1       -> x2 = PC (0x1004) + 0x1000 = 0x2004
        ]);

        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(1), 0x1234_5000);

        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(2), 0x2004);
    }

    #[test]
    fn test_jumps_jal_and_jalr() {
        let (mut cpu, mut bus) = setup_env(&[
            // 0x1000: jal x1, 8 -> прыжок на 0x1008, x1 = 0x1004
            0x0080_00EF,
            // 0x1004: addi x2, x0, 99 (должна быть пропущена)
            0x0630_0113,
            // 0x1008: jalr x3, x1, 1 -> прыжок на (x1 + 1) & !1 = 0x1004, x3 = 0x100C
            0x0010_81E7,
        ]);

        // 1. Выполняем JAL
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(1), 0x1004);
        assert_eq!(cpu.pc, 0x1008);
        assert_eq!(cpu.read_reg(2), 0); // Инструкция по адресу 0x1004 не выполнялась

        // 2. Выполняем JALR (проверяем также сброс младшего бита адреса: 0x1004 + 1 -> 0x1004)
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(3), 0x100C);
        assert_eq!(cpu.pc, 0x1004);

        // 3. Выполняем ADDI по адресу 0x1004, куда вернулись через JALR
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(2), 99);
    }

    #[test]
    fn test_conditional_branches() {
        let (mut cpu, mut bus) = setup_env(&[
            // 0x1000: beq x1, x2, 8 (не сработает, т.к. -5 != 10) -> PC = 0x1004
            0x0020_8463,
            // 0x1004: bne x1, x2, 8 (сработает, прыжок на 0x100C)
            0x0020_9463,
            // 0x1008: addi x3, x0, 1 (пропускается)
            0x0010_0193,
            // 0x100C: blt x1, x2, 8 (signed: -5 < 10, сработает -> прыжок на 0x1014)
            0x0020_C463,
            // 0x1010: addi x3, x0, 2 (пропускается)
            0x0020_0193,
            // 0x1014: bgeu x1, x2, 8 (unsigned: 0xFFFFFFFB >= 10, сработает -> прыжок на 0x101C)
            0x0020_F463,
            // 0x1018: addi x3, x0, 3 (пропускается)
            0x0030_0193,
            // 0x101C: addi x4, x0, 42 (финиш)
            0x02A0_0213,
        ]);

        cpu.write_reg(1, (-5i32) as u32);
        cpu.write_reg(2, 10);

        // BEQ (не взят)
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x1004);

        // BNE (взят)
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x100C);

        // BLT (взят)
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x1014);

        // BGEU (взят)
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x101C);

        // Финальный ADDI
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(3), 0); // Ни один из пропущенных ADDI не сработал
        assert_eq!(cpu.read_reg(4), 42);
    }

    #[test]
    fn test_loads_and_stores_with_sign_extension() {
        let (mut cpu, mut bus) = setup_env(&[
            // Записываем данные по базовому адресу в x1 (0x1100):
            0x0020_A023, // sw  x2, 0(x1)
            0x0020_9223, // sh  x2, 4(x1)
            0x0020_8423, // sb  x2, 8(x1)
            // Читаем обратно с проверкой знакового и беззнакового расширения:
            0x0000_A183, // lw  x3, 0(x1)
            0x0040_9203, // lh  x4, 4(x1)  (знаковое)
            0x0040_D283, // lhu x5, 4(x1)  (беззнаковое)
            0x0080_8303, // lb  x6, 8(x1)  (знаковое)
            0x0080_C383, // lbu x7, 8(x1)  (беззнаковое)
        ]);

        cpu.write_reg(1, 0x1100);
        // Младший байт 0x80 (-128), младшее полуслово 0xFF80 (-128)
        cpu.write_reg(2, 0xDEAD_FF80);

        for _ in 0..8 {
            cpu.step(&mut bus).unwrap();
        }

        assert_eq!(cpu.read_reg(3), 0xDEAD_FF80); // lw
        assert_eq!(cpu.read_reg(4), 0xFFFF_FF80); // lh: знаковое расширение 0xFF80
        assert_eq!(cpu.read_reg(5), 0x0000_FF80); // lhu: беззнаковое расширение 0xFF80
        assert_eq!(cpu.read_reg(6), 0xFFFF_FF80); // lb: знаковое расширение 0x80
        assert_eq!(cpu.read_reg(7), 0x0000_0080); // lbu: беззнаковое расширение 0x80
    }

    #[test]
    fn test_illegal_instruction_error() {
        // 0xFFFF_FFFF не является валидной инструкцией RV32I
        let (mut cpu, mut bus) = setup_env(&[0xFFFF_FFFF]);

        let res = cpu.step(&mut bus);
        assert_eq!(res, Err(CpuError::IllegalInstruction(0xFFFF_FFFF)));
        assert_eq!(cpu.pc, 0x1000); // PC не должен измениться при ошибке
    }

    #[test]
    fn test_instruction_fetch_bus_error() {
        let (_, mut bus) = setup_env(&[]);
        // Ставим PC в неразмеченную область памяти
        let mut cpu = Cpu::new(0x9000_0000);

        let res = cpu.step(&mut bus);
        assert!(matches!(res, Err(CpuError::Bus(_))));
    }

    #[test]
    fn test_ecall_and_ebreak_act_as_nop() {
        let (mut cpu, mut bus) = setup_env(&[
            0x0000_0073, // ecall
            0x0010_0073, // ebreak
            0x02A0_0093, // addi x1, x0, 42
        ]);

        // ECALL просто сдвигает PC на 4
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x1004);

        // EBREAK просто сдвигает PC на 4
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x1008);

        // Следующая инструкция штатно выполняется
        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(1), 42);
    }

    #[test]
    fn test_custom_instruction_without_coprocessor_fails() {
        // Opcode 0x0B (0001011) — стандартный слот custom-0 в RISC-V
        let raw_custom = 0x0000_000B;
        let (mut cpu, mut bus) = setup_env(&[raw_custom]);

        let res = cpu.step(&mut bus);
        assert_eq!(res, Err(CpuError::IllegalInstruction(raw_custom)));
        assert_eq!(cpu.pc, 0x1000); // При ошибке PC остается на месте
    }

    #[test]
    fn test_custom_instruction_with_coprocessor_and_x0_protection() {
        // Тестовый сопроцессор, который пытается испортить x0, меняет x1 и делает переход
        struct DirtyCoproc;
        impl Coprocessor for DirtyCoproc {
            fn execute(
                &mut self,
                opcode: u8,
                _raw: u32,
                regs: &mut [Reg; 32],
                next_pc: &mut Reg,
                _bus: &mut dyn crate::bus::BusDevice,
            ) -> Result<(), CpuError> {
                assert_eq!(opcode, 0x0B);
                regs[0] = 0xDEAD_BEEF; // Пытаемся перезаписать x0 напрямую через массив
                regs[1] = 0xCAFE_BABE; // Записываем результат в x1
                *next_pc = 0x1010; // Изменяем поток управления
                Ok(())
            }
        }

        let raw_custom = 0x0000_000B;
        let (mut cpu, mut bus) = setup_env(&[raw_custom]);
        cpu.attach_coprocessor(Box::new(DirtyCoproc));

        cpu.step(&mut bus).unwrap();

        // Проверяем, что CPU принудительно сбросил regs[0] обратно в 0 после вызова сопроцессора
        assert_eq!(cpu.read_reg(0), 0);
        assert_eq!(cpu.regs[0], 0);
        assert_eq!(cpu.read_reg(1), 0xCAFE_BABE);
        assert_eq!(cpu.pc, 0x1010);
    }

    #[test]
    fn test_remaining_branches_bge_and_bltu() {
        let (mut cpu, mut bus) = setup_env(&[
            // 0x1000: bge x1, x1, 8 (signed: -5 >= -5 -> истина, прыжок на 0x1008)
            0x0010_D463,
            // 0x1004: addi x3, x0, 1 (пропускается)
            0x0010_0193,
            // 0x1008: bltu x2, x1, 8 (unsigned: 10 < 0xFFFFFFFB -> истина, прыжок на 0x1010)
            0x0011_6463,
            // 0x100C: addi x3, x0, 2 (пропускается)
            0x0020_0193,
            // 0x1010: bge x1, x2, 8 (signed: -5 >= 10 -> ложь, PC = 0x1014)
            0x0020_D463,
            // 0x1014: addi x4, x0, 77 (выполняется)
            0x04D0_0213,
        ]);

        cpu.write_reg(1, (-5i32) as u32);
        cpu.write_reg(2, 10);

        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x1008); // BGE сработал на равенстве отрицательных чисел

        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x1010); // BLTU сработал

        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x1014); // BGE не сработал

        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.read_reg(3), 0);
        assert_eq!(cpu.read_reg(4), 77);
    }

    #[test]
    fn test_jalr_same_rd_and_rs1_edge_case() {
        // Важный корнер-кейс: jalr x1, x1, 0
        // Процессор обязан сначала вычислить target из СТАРОГО значения x1,
        // и только потом записать в x1 адрес возврата (PC + 4).
        let (mut cpu, mut bus) = setup_env(&[
            0x0000_80E7, // jalr x1, x1, 0
        ]);

        cpu.write_reg(1, 0x1020);

        cpu.step(&mut bus).unwrap();
        assert_eq!(cpu.pc, 0x1020, "PC должен прыгнуть по старому значению x1");
        assert_eq!(
            cpu.read_reg(1),
            0x1004,
            "В x1 должен сохраниться адрес возврата"
        );
    }

    #[test]
    fn test_shift_amount_masks_only_lower_5_bits() {
        // По спецификации RV32I сдвиг берет только младшие 5 бит регистра rs2 (маска 0x1F).
        // Поэтому сдвиг на 33 (0b10_0001) должен сработать как сдвиг на 1.
        let (mut cpu, mut bus) = setup_env(&[
            0x0020_91B3, // sll x3, x1, x2
            0x0020_D233, // srl x4, x1, x2
        ]);

        cpu.write_reg(1, 0b1010);
        cpu.write_reg(2, 33); // 33 & 0x1F == 1

        cpu.step(&mut bus).unwrap();
        cpu.step(&mut bus).unwrap();

        assert_eq!(cpu.read_reg(3), 0b10100); // 10 << 1 = 20
        assert_eq!(cpu.read_reg(4), 0b0101); // 10 >> 1 = 5
    }

    #[test]
    fn test_backward_branch_loop() {
        // Небольшой реальный цикл: уменьшаем счетчик x1 от 3 до 0
        // 0x1000: addi x1, x1, -1 (0xFFF0_8093)
        // 0x1004: bne  x1, x0, -4 (0xFE00_9EE3) -> прыжок назад на 0x1000
        let (mut cpu, mut bus) = setup_env(&[0xFFF0_8093, 0xFE00_9EE3]);

        cpu.write_reg(1, 3);

        // 3 итерации цикла (по 2 инструкции в каждой = 6 шагов)
        for _ in 0..6 {
            cpu.step(&mut bus).unwrap();
        }

        assert_eq!(cpu.read_reg(1), 0);
        assert_eq!(cpu.pc, 0x1008); // Вышли из цикла
    }

    #[test]
    fn test_negative_offset_load_and_store() {
        let (mut cpu, mut bus) = setup_env(&[
            0xFE20_A823, // sw x2, -16(x1) -> адрес 0x1100 - 16 = 0x10F0
            0xFF00_A183, // lw x3, -16(x1) -> читаем с 0x10F0
        ]);

        cpu.write_reg(1, 0x1100);
        cpu.write_reg(2, 0x1234_5678);

        cpu.step(&mut bus).unwrap();
        cpu.step(&mut bus).unwrap();

        assert_eq!(cpu.read_reg(3), 0x1234_5678);
    }

    #[test]
    fn test_load_and_store_bus_fault_leaves_pc_unchanged() {
        let (mut cpu, mut bus) = setup_env(&[
            0x0000_A183, // lw x3, 0(x1)
            0x0020_A023, // sw x2, 0(x1)
        ]);

        // Указываем адрес за пределами размеченной RAM (0x1000..0x2000)
        cpu.write_reg(1, 0x5000_0000);

        // Ошибка чтения по шине при выполнении LW
        let res_load = cpu.step(&mut bus);
        assert!(matches!(res_load, Err(CpuError::Bus(_))));
        assert_eq!(
            cpu.pc, 0x1000,
            "PC не должен меняться при ошибке памяти на Load"
        );

        // Передвигаем PC вручную на SW и проверяем ошибку записи
        cpu.pc = 0x1004;
        let res_store = cpu.step(&mut bus);
        assert!(matches!(res_store, Err(CpuError::Bus(_))));
        assert_eq!(
            cpu.pc, 0x1004,
            "PC не должен меняться при ошибке памяти на Store"
        );
    }

    #[test]
    fn test_arithmetic_wrapping_overflow() {
        // В RISC-V целочисленное переполнение в ADD/SUB не вызывает исключений, а оборачивается (wrapping)
        let (mut cpu, mut bus) = setup_env(&[
            0x0020_81B3, // add x3, x1, x2
            0x4022_0233, // sub x4, x4, x2 (0 - 1 = 0xFFFF_FFFF)
        ]);

        cpu.write_reg(1, u32::MAX);
        cpu.write_reg(2, 1);
        cpu.write_reg(4, 0);

        cpu.step(&mut bus).unwrap();
        cpu.step(&mut bus).unwrap();

        assert_eq!(cpu.read_reg(3), 0);
        assert_eq!(cpu.read_reg(4), u32::MAX);
    }
}
