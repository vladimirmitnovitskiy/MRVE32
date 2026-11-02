/*
 * Test firmware for the mrve32 runner: freestanding C, QEMU-virt devices.
 *
 * Prints a greeting and fib(20), echoes UART input, then acts on the first
 * input byte: 'f' exits with status 3 through the test finisher, 'c'
 * executes an illegal instruction with no trap handler (mtvec = 0), 'w'
 * sleeps in wfi with interrupts disabled; anything else exits with 0 if
 * fib(20) is correct.
 */
#include <stdint.h>

#define UART ((volatile uint8_t *)0x10000000)
#define UART_LSR_DR 0x01
#define UART_LSR_THRE 0x20
#define FINISHER (*(volatile uint32_t *)0x00100000)

static void uart_putc(char c)
{
    while (!(UART[5] & UART_LSR_THRE))
        ;
    UART[0] = (uint8_t)c;
}

static void uart_puts(const char *s)
{
    while (*s)
        uart_putc(*s++);
}

static void uart_putu(uint32_t v)
{
    char buf[11];
    int i = sizeof buf - 1;
    buf[i] = '\0';
    do {
        buf[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v);
    uart_puts(&buf[i]);
}

static int uart_getc(void)
{
    return (UART[5] & UART_LSR_DR) ? UART[0] : -1;
}

static __attribute__((noreturn)) void finish(uint32_t status)
{
    FINISHER = status ? (status << 16) | 0x3333 : 0x5555;
    for (;;)
        ;
}

static uint32_t fib(uint32_t n)
{
    uint32_t a = 0, b = 1;
    while (n--) {
        uint32_t t = a + b;
        a = b;
        b = t;
    }
    return a;
}

int main(void)
{
    uart_puts("Hello from MRVE32!\n");
    uint32_t f = fib(20);
    uart_puts("fib(20) = ");
    uart_putu(f);
    uart_putc('\n');

    int first = -1, c;
    while ((c = uart_getc()) >= 0) {
        if (first < 0) {
            first = c;
            uart_puts("input: ");
        }
        uart_putc((char)c);
    }

    switch (first) {
    case 'f':
        finish(3);
    case 'c':
        __asm__ volatile(".word 0");
        break;
    case 'w':
        __asm__ volatile("csrw mie, zero\n wfi");
        break;
    }
    finish(f == 6765 ? 0 : 1);
}
