/*
 * Copyright (c) 2025 Raiki Tamura.
 * SPDX-License-Identifier: MIT OR Apache-2.0 WITH LLVM-exception
 */

// Kafu Attributes for Aspect-Oriented Programming
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

// The KAFU_DEST attribute.
#define KAFU_DEST(ident, dest)                                                                     \
  __asm__(".section .custom_section..kafu_dest." #ident "." dest ",\"\",@\n");

// The KAFU_EXPORT attribute.
// Do not attach `static` to the function to make its symbol visible to the runtime.
// For C++: extern "C" is required to avoid name mangling.
#ifdef __cplusplus
#define KAFU_EXPORT(ident) extern "C" __attribute__((used, export_name(#ident), noinline))
#else
#define KAFU_EXPORT(ident) __attribute__((used, export_name(#ident), noinline))
#endif

#ifdef __cplusplus
}
#endif
