#include "wasi_nn_backend.h"
#include "wasi_nn_types.h"
#include "kafu_helper.h"
#include "kafu.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

static uint8_t *read_file_to_buf(const char *filename, uint32_t *out_size) {
  if (!filename || !out_size)
    return NULL;

  FILE *fp = fopen(filename, "rb");
  if (!fp)
    return NULL;

  if (fseek(fp, 0, SEEK_END) != 0) {
    fclose(fp);
    return NULL;
  }
  long size_long = ftell(fp);
  if (size_long <= 0) {
    fclose(fp);
    return NULL;
  }
  if (fseek(fp, 0, SEEK_SET) != 0) {
    fclose(fp);
    return NULL;
  }

  uint32_t size = (uint32_t)size_long;
  uint8_t *buf = (uint8_t *)malloc(size);
  if (!buf) {
    fclose(fp);
    return NULL;
  }

  size_t nread = fread(buf, 1, size, fp);
  fclose(fp);
  if (nread != size) {
    free(buf);
    return NULL;
  }

  *out_size = size;
  return buf;
}

static wasi_nn_error load_graph_from_onnx_file(const char *filename, graph *out_graph) {
  if (!filename || !out_graph) {
    return WASI_NN_ERROR_NAME(invalid_argument);
  }
  uint32_t model_size = 0;
  uint8_t *model_buf = read_file_to_buf(filename, &model_size);
  if (!model_buf) {
    printf("Failed to read model file: %s\n", filename);
    return WASI_NN_ERROR_NAME(not_found);
  }
  printf("Read ONNX model, size in bytes: %u\n", model_size);

  graph_builder model = {
      .buf = model_buf,
      .size = model_size,
  };

  wasi_nn_error err = wasi_nn_load(&model, 1, onnx, cpu, out_graph);
  free(model_buf);
  if (err != WASI_NN_ERROR_NAME(success)) {
    printf("Failed to load graph: %d\n", err);
    return err;
  }
  printf("Loaded graph into wasi-nn\n");
  return WASI_NN_ERROR_NAME(success);
}

// Compute softmax function
static void compute_softmax(const float *input, float *output, int size) {
  float max_val = input[0];
  for (int i = 1; i < size; i++) {
    if (input[i] > max_val) {
      max_val = input[i];
    }
  }

  float sum_exp = 0.0f;
  for (int i = 0; i < size; i++) {
    output[i] = expf(input[i] - max_val); // Subtract max value for numerical stability
    sum_exp += output[i];
  }

  for (int i = 0; i < size; i++) {
    output[i] /= sum_exp;
  }
}

// Run inference on the edge node and return the result label as a malloc'd string
KAFU_DEST(run_inference, "edge")
KAFU_EXPORT(run_inference)
int run_inference(char **out_result_label) {
  const char *MODEL_PATH = "fixture/models/squeezenet1.1-7.onnx";
  const char *LABELS_PATH = "fixture/labels/squeezenet1.1-7.txt";
  const char *IMG_PATH = "fixture/images/dog.jpg";

  char *labels_buf = NULL;
  uint8_t *image_tensor_buf = NULL;
  uint8_t *output_buffer = NULL;
  float *softmax_output = NULL;

  graph g;
  wasi_nn_error err = load_graph_from_onnx_file(MODEL_PATH, &g);
  if (err != WASI_NN_ERROR_NAME(success)) {
    goto cleanup;
  }

  graph_execution_context exec_ctx;
  err = wasi_nn_init_execution_context(g, &exec_ctx);
  if (err != WASI_NN_ERROR_NAME(success)) {
    printf("Failed to init execution context: %d\n", err);
    goto cleanup;
  }
  printf("Created wasi-nn execution context.\n");

  // Load SqueezeNet 1000 labels used for classification
  FILE *labels_fp = fopen(LABELS_PATH, "r");
  if (!labels_fp) {
    printf("Failed to open labels file\n");
    err = WASI_NN_ERROR_NAME(not_found);
    goto cleanup;
  }
  labels_buf = (char *)malloc(1000 * 100);
  if (!labels_buf) {
    fclose(labels_fp);
    printf("Failed to allocate memory for labels\n");
    err = WASI_NN_ERROR_NAME(missing_memory);
    goto cleanup;
  }
  size_t labels_nread = fread(labels_buf, 1, 1000 * 100, labels_fp);
  fclose(labels_fp);
  if (labels_nread == 0) {
    printf("Failed to read labels file\n");
    err = WASI_NN_ERROR_NAME(runtime_error);
    goto cleanup;
  }
  // Store labels as an array to preserve order
  char *labels[1000];
  int label_count = 0;
  char *label = strtok(labels_buf, "\n");
  while (label && label_count < 1000) {
    labels[label_count] = label;
    label_count++;
    label = strtok(NULL, "\n");
  }
  printf("Read ONNX Labels, # of labels: %d\n", label_count);

  // Prepare WASI-NN tensor - Tensor data is always a bytes vector
  uint32_t dim_data[] = {1, 3, 224, 224};
  tensor_dimensions dimensions = {.buf = (uint32_t *)&dim_data, .size = 4};
  image_tensor_buf = (uint8_t *)malloc(224 * 224 * 3 * 4);
  if (!image_tensor_buf) {
    printf("Failed to allocate memory for image tensor\n");
    err = WASI_NN_ERROR_NAME(missing_memory);
    goto cleanup;
  }
  uint32_t nwritten;
  image_to_tensor(IMG_PATH, strlen(IMG_PATH), 224, 224, image_tensor_buf, &nwritten);
  if (nwritten != 224 * 224 * 3 * 4) {
    printf("Failed to convert image to tensor\n");
    err = WASI_NN_ERROR_NAME(runtime_error);
    goto cleanup;
  }
  //printf("Image tensor buffer: %p\n", image_tensor_buf);
  tensor_data data = {.buf = image_tensor_buf, .size = nwritten};
  tensor input_tensor = {
      .dimensions = dimensions,
      .type = WASI_NN_TYPE_NAME(fp32),
      .data = data,
  };
  err = wasi_nn_set_input(exec_ctx, 0, &input_tensor);
  if (err != WASI_NN_ERROR_NAME(success)) {
    printf("Failed to set input: %d\n", err);
    goto cleanup;
  }
  err = wasi_nn_compute(exec_ctx);
  if (err != WASI_NN_ERROR_NAME(success)) {
    printf("Failed to compute: %d\n", err);
    goto cleanup;
  }
  // output shape is [1, 1000, 1, 1] = 1000 floats = 4000 bytes
  uint32_t output_buffer_max_size = 1000 * sizeof(float);
  output_buffer = (uint8_t *)malloc(output_buffer_max_size);
  if (!output_buffer) {
    printf("Failed to allocate memory for output buffer\n");
    err = WASI_NN_ERROR_NAME(missing_memory);
    goto cleanup;
  }
  uint32_t output_buffer_size;
  err = wasi_nn_get_output(exec_ctx, 0, output_buffer, output_buffer_max_size, &output_buffer_size);
  if (err != WASI_NN_ERROR_NAME(success)) {
    printf("Failed to get output: %d\n", err);
    goto cleanup;
  }
  printf("Executed graph inference\n");

  float *output_buffer_f32 = (float *)output_buffer;

  // Post-Processing requirement: compute softmax to inferencing output
  softmax_output = (float *)malloc(1000 * sizeof(float));
  if (!softmax_output) {
    printf("Failed to allocate memory for softmax output\n");
    err = WASI_NN_ERROR_NAME(missing_memory);
    goto cleanup;
  }
  compute_softmax(output_buffer_f32, softmax_output, 1000);

  // Find the index of the maximum probability
  int max_index = 0;
  float max_prob = softmax_output[0];
  for (int i = 1; i < 1000; i++) {
    if (softmax_output[i] > max_prob) {
      max_prob = softmax_output[i];
      max_index = i;
    }
  }

  *out_result_label = strdup(labels[max_index]);
  if (!*out_result_label) {
    printf("Failed to allocate memory for result label\n");
    err = WASI_NN_ERROR_NAME(missing_memory);
    goto cleanup;
  }

  fflush(stdout);

cleanup:
  free(labels_buf);
  free(image_tensor_buf);
  free(output_buffer);
  free(softmax_output);

  return (err == WASI_NN_ERROR_NAME(success)) ? 0 : 1;
}

KAFU_DEST(report_inference_result, "cloud")
KAFU_EXPORT(report_inference_result)
void report_inference_result(char *result_label) {
  printf("Inference result: %s\n", result_label);
}

int main() {
  printf("Starting Kafu main\n");
  char *result_label = NULL;
  int err = run_inference(&result_label);
  if (err != 0) {
    printf("Failed to run inference: %d\n", err);
    return 1;
  }
  report_inference_result(result_label);
  free(result_label);

  return 0;
}
