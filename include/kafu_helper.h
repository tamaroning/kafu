/*
 * Copyright (c) 2025 Raiki Tamura.
 * SPDX-License-Identifier: MIT OR Apache-2.0 WITH LLVM-exception
 */

// Kafu Helper API
// TODO: Generate this header from witx/kafu.witx using wit-bindgen

#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define KAFU_ERROR_NAME(name) kafu_##name
#define KAFU_ERROR_TYPE kafu_error

typedef enum {
  KAFU_ERROR_NAME(success) = 0,
  KAFU_ERROR_NAME(invalid_argument),
  KAFU_ERROR_NAME(invalid_encoding),
  KAFU_ERROR_NAME(missing_memory),
  KAFU_ERROR_NAME(busy),
  KAFU_ERROR_NAME(runtime_error),
  KAFU_ERROR_NAME(unsupported_operation),
  KAFU_ERROR_NAME(too_large),
  KAFU_ERROR_NAME(not_found),
} KAFU_ERROR_TYPE;

__attribute__((import_module("kafu_helper"), import_name("image_to_tensor"))) kafu_error
image_to_tensor(const char *path, int pathlen, uint32_t height, uint32_t width, uint8_t *output,
                uint32_t *nwritten);

#ifdef __cplusplus
}
#endif