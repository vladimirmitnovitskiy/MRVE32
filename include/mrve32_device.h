/*
 * MRVE32 device ABI, version 1.
 *
 * Lets a memory-mapped device built separately from the emulator (in C,
 * C++, Rust, ...) be attached to it. Mirrors `mrve32::plugin::RawDevice`;
 * see docs/devices.md for the full contract.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#ifndef MRVE32_DEVICE_H
#define MRVE32_DEVICE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define MRVE32_DEVICE_ABI_VERSION 1u

/* Name of the PyCapsule holding a `mrve32_device *` (Python transport). */
#define MRVE32_DEVICE_CAPSULE_NAME "mrve32.device.v1"

/* Callback status codes. */
#define MRVE32_STATUS_OK 0
#define MRVE32_STATUS_ADDRESS_NOT_FOUND 1
#define MRVE32_STATUS_INVALID_ACCESS 2
#define MRVE32_STATUS_BUSY 3

/*
 * offset: byte offset inside the device window.
 * width:  1, 2 or 4 bytes. Values are little-endian, zero-extended.
 */
typedef int32_t (*mrve32_read_fn)(void *ctx, uint32_t offset, uint32_t width, uint32_t *value);
typedef int32_t (*mrve32_write_fn)(void *ctx, uint32_t offset, uint32_t width, uint32_t value);
/* Called once per CPU step with the elapsed CPU cycles. May be NULL. */
typedef void (*mrve32_tick_fn)(void *ctx, uint64_t cycles);
/* Asserted interrupt lines in `mip` layout (MEIP = 1 << 11, ...). May be NULL. */
typedef uint32_t (*mrve32_irq_lines_fn)(const void *ctx);
/* Frees the device. Called exactly once by the owner. */
typedef void (*mrve32_destroy_fn)(void *ctx);

typedef struct mrve32_device {
    uint32_t abi_version; /* MRVE32_DEVICE_ABI_VERSION */
    uint32_t size;        /* address window size in bytes, non-zero */
    void *ctx;            /* device state; NULL once taken by the emulator */
    mrve32_read_fn read;
    mrve32_write_fn write;
    mrve32_tick_fn tick;
    mrve32_irq_lines_fn irq_lines;
    mrve32_destroy_fn destroy;
} mrve32_device;

/*
 * Python transport: a device class implements `__mrve32_device__(self)`
 * returning PyCapsule_New(dev, MRVE32_DEVICE_CAPSULE_NAME, destructor),
 * where `dev` is a heap-allocated mrve32_device. The emulator copies the
 * table and sets `dev->ctx = NULL`; the capsule destructor must free `dev`
 * and call `dev->destroy(dev->ctx)` only if `dev->ctx != NULL`.
 */

#ifdef __cplusplus
}
#endif

#endif /* MRVE32_DEVICE_H */
