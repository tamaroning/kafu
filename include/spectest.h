/*
 * Copyright (c) 2025 Raiki Tamura.
 * SPDX-License-Identifier: MIT OR Apache-2.0 WITH LLVM-exception
 */

// WebAssembly Spectest API
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// NOTE:
// - This header provides declarations compatible with the "spectest" module
//   used by WebAssembly spec tests and linked by Rust's `wast` tooling.
// - `import_module` / `import_name` are Clang WebAssembly attributes.
//   If your toolchain does not support them, you may need to provide these
//   imports via your linker/runner instead.

#if defined(__clang__) && defined(__has_attribute)
#if __has_attribute(import_module) && __has_attribute(import_name)
#define SPECTEST_IMPORT(module, name) __attribute__((import_module(module), import_name(name)))
#else
#define SPECTEST_IMPORT(module, name)
#endif
#else
#define SPECTEST_IMPORT(module, name)
#endif

SPECTEST_IMPORT("spectest", "print") void spectest_print(void);

SPECTEST_IMPORT("spectest", "print_i32") void spectest_print_i32(int32_t v);

SPECTEST_IMPORT("spectest", "print_i64") void spectest_print_i64(int64_t v);

SPECTEST_IMPORT("spectest", "print_f32") void spectest_print_f32(float v);

SPECTEST_IMPORT("spectest", "print_f64") void spectest_print_f64(double v);

// Composite print functions used by the official spec tests.
SPECTEST_IMPORT("spectest", "print_i32_f32") void spectest_print_i32_f32(int32_t i, float f);
SPECTEST_IMPORT("spectest", "print_f64_f64") void spectest_print_f64_f64(double a, double b);

#undef SPECTEST_IMPORT

#ifdef __cplusplus
}
#endif
