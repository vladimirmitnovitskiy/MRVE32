// rvmodel_macros.h
// RVMODEL macros for MRVE32 running ELFs with the `mrve32` CLI on its default
// QEMU-virt-like memory map:
//   RAM 0x80000000, sifive,test0 finisher 0x00100000, NS16550A UART
//   0x10000000, CLINT 0x02000000; nothing is mapped at 0x00000100.
// SPDX-License-Identifier: Apache-2.0

#ifndef _RVMODEL_MACROS_H
#define _RVMODEL_MACROS_H

#define RVMODEL_DATA_SECTION

// Standard M-mode CSRs (mstatus, mtvec, mepc, mcause, mtval, mie, mip, ...).
#define STANDARD_SM_SUPPORTED

.EQU MRVE32_FINISHER,  0x00100000
.EQU MRVE32_UART,      0x10000000
.EQU MRVE32_UART_IER,  (MRVE32_UART + 1)
.EQU MRVE32_UART_LSR,  (MRVE32_UART + 5)

##### Access Fault #####

// Unmapped: loads, stores and fetches raise access faults.
#define RVMODEL_ACCESS_FAULT_ADDRESS 0x00000100

##### TERMINATION #####

// The finisher stops the emulator: 0x5555 exits 0, (code << 16) | 0x3333
// exits with `code`.
#define RVMODEL_HALT_PASS      \
  li t0, MRVE32_FINISHER      ;\
  li t1, 0x5555               ;\
  sw t1, 0(t0)                ;\
  j .                         ;

#define RVMODEL_HALT_FAIL      \
  li t0, MRVE32_FINISHER      ;\
  li t1, 0x13333              ;\
  sw t1, 0(t0)                ;\
  j .                         ;

##### IO #####

// Polls LSR.THRE, then writes THR (bytes go to the CLI's stdout).
#define RVMODEL_IO_WRITE_STR(_R1, _R2, _R3, _STR_PTR)  \
1:                                                    ;\
  lbu  _R1, 0(_STR_PTR)                               ;\
  beqz _R1, 3f                                        ;\
  li   _R2, MRVE32_UART_LSR                           ;\
2:                                                    ;\
  lbu  _R3, 0(_R2)                                    ;\
  andi _R3, _R3, 0x20                                 ;\
  beqz _R3, 2b                                        ;\
  li   _R2, MRVE32_UART                               ;\
  sb   _R1, 0(_R2)                                    ;\
  addi _STR_PTR, _STR_PTR, 1                          ;\
  j    1b                                             ;\
3:

##### Machine Timer #####

#define RVMODEL_MTIME_ADDRESS    0x0200BFF8
#define RVMODEL_MTIMECMP_ADDRESS 0x02004000
#define RVMODEL_MSIP_ADDRESS     0x02000000

##### Machine Interrupts #####

// Interrupts are taken before the next instruction.
#define RVMODEL_INTERRUPT_LATENCY 10
#define RVMODEL_TIMER_INT_SOON_DELAY 1000

// There is no PLIC: the UART drives mip.MEIP directly. Enabling the THR-empty
// interrupt (IER.ETBEI) raises it at once; disabling it lowers it.
#define RVMODEL_SET_MEXT_INT(_R1, _R2)  \
  li _R1, MRVE32_UART_IER              ;\
  li _R2, 0x02                         ;\
  sb _R2, 0(_R1)                       ;

#define RVMODEL_CLR_MEXT_INT(_R1, _R2)  \
  li _R1, MRVE32_UART_IER              ;\
  sb zero, 0(_R1)                      ;

#endif // _RVMODEL_MACROS_H
