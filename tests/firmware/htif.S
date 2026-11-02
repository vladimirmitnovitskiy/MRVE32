# HTIF and signature test firmware for tests/cli.rs (`--htif`, `--signature`):
# prints "hi\n" on the HTIF console, fills the signature region and exits
# with the code in exit_code (3, a failure, so the code path is visible).
    .section .text.start, "ax"
    .globl _start
_start:
    la      t0, tohost
    la      t1, message
1:  lbu     t2, 0(t1)
    beqz    t2, 2f
    sw      t2, 0(t0)
    li      t3, 0x01010000      # device 1 (console), command 1 (putchar)
    sw      t3, 4(t0)
    addi    t1, t1, 1
    j       1b
2:  la      t1, begin_signature
    li      t2, 0xcafef00d
    sw      t2, 0(t1)
    li      t2, 0x12345678
    sw      t2, 4(t1)
    lw      t2, exit_code
    slli    t2, t2, 1
    ori     t2, t2, 1
    sw      t2, 0(t0)
    sw      zero, 4(t0)
3:  j       3b

    .section .rodata
message:    .asciz "hi\n"
exit_code:  .word 3

    .section .data
    .balign 8
    .globl tohost
tohost:     .dword 0
    .globl begin_signature, end_signature
    .balign 16
begin_signature:
    .word 0, 0, 0xdeadbeef
end_signature:
