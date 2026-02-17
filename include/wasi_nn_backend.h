/*
 * Copyright (c) 2019 Intel Corporation.  All rights reserved.
 * Copyright (c) 2025 Raiki Tamura.
 * SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
 */

#ifndef WASI_NN_BACKEND_H
#define WASI_NN_BACKEND_H

#include "wasi_nn_types.h"

#ifdef __cplusplus
extern "C" {
#endif

__attribute__((import_module("wasi_ephemeral_nn"), import_name("load")))
__attribute__((visibility("default"))) wasi_nn_error
wasi_nn_load(graph_builder *builder_array_buf, int builder_array_size, graph_encoding encoding,
             execution_target target, graph *g);

__attribute__((import_module("wasi_ephemeral_nn"), import_name("load_by_name")))
__attribute__((visibility("default"))) wasi_nn_error
wasi_nn_load_by_name(const char *name, uint32_t namelen, graph *g);

__attribute__((import_module("wasi_ephemeral_nn"), import_name("load_by_name_with_config")))
__attribute__((visibility("default"))) wasi_nn_error
wasi_nn_load_by_name_with_config(const char *name, uint32_t namelen, const char *config,
                                 uint32_t config_len, graph *g);

__attribute__((import_module("wasi_ephemeral_nn"), import_name("init_execution_context")))
__attribute__((visibility("default"))) wasi_nn_error
wasi_nn_init_execution_context(graph g, graph_execution_context *exec_ctx);

__attribute__((import_module("wasi_ephemeral_nn"), import_name("set_input")))
__attribute__((visibility("default"))) wasi_nn_error
wasi_nn_set_input(graph_execution_context exec_ctx, uint32_t index, tensor *input_tensor);

__attribute__((import_module("wasi_ephemeral_nn"), import_name("compute")))
__attribute__((visibility("default"))) wasi_nn_error
wasi_nn_compute(graph_execution_context exec_ctx);

__attribute__((import_module("wasi_ephemeral_nn"), import_name("get_output")))
__attribute__((visibility("default"))) wasi_nn_error
wasi_nn_get_output(graph_execution_context exec_ctx, uint32_t index, uint8_t *output_buffer,
                   uint32_t output_buffer_max_size, uint32_t *output_buffer_size);

#ifdef __cplusplus
}
#endif

#endif /* WASI_NN_BACKEND_H */