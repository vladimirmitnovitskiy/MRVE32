// rvtest_config.h for the local (no Sail, no UDB) flow of scripts/arch-test.sh.
// The ACT4 framework generates this file from ../mrve32-rv32imac/mrve32-rv32imac.yaml
// with the UDB gem; this hand-written copy defines the subset the selected
// unprivileged tests and the test environment use, and must agree with it.
// SPDX-License-Identifier: Apache-2.0

#ifndef RVTEST_CONFIG_H
#define RVTEST_CONFIG_H

#define UDB_MXLEN 32

#define I_SUPPORTED
#define M_SUPPORTED
#define ZMMUL_SUPPORTED
#define A_SUPPORTED
#define ZAAMO_SUPPORTED
#define ZALRSC_SUPPORTED
#define C_SUPPORTED
#define ZCA_SUPPORTED
#define ZICSR_SUPPORTED
#define ZIFENCEI_SUPPORTED
#define ZICNTR_SUPPORTED
#define ZIHINTNTL_SUPPORTED
#define ZIHINTPAUSE_SUPPORTED
#define SM_SUPPORTED
#define SM1P12P0_SUPPORTED

#define UDB_NUM_PMP_ENTRIES 0
#define UDB_MISALIGNED_LDST

#define UDB_MEI_INTR_IMPL
#define UDB_MSI_INTR_IMPL
#define UDB_MTI_INTR_IMPL

#define UDB_MTVEC_MODES_0
#define UDB_MTVEC_MODES_1
#define UDB_MTVEC_BASE_ALIGNMENT_DIRECT 4
#define UDB_MTVEC_BASE_ALIGNMENT_VECTORED 4

#define UDB_REPORT_ENCODING_IN_MTVAL_ON_ILLEGAL_INSTRUCTION
#define UDB_MCOUNTINHIBIT_IMPLEMENTED
// No UDB_TIME_CSR_IMPLEMENTED: reads of time are emulated from mtime.

#endif // RVTEST_CONFIG_H
