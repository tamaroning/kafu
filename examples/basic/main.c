#include "kafu.h"
#include <stdio.h>
#include <spectest.h>

void f();
void g();

int main() {
  f();
}

KAFU_DEST(f, "cloud1")
KAFU_EXPORT(f)
void f() {
  printf("Hello, from cloud\n");
  fflush(stdout);
  g();
  printf("Hello, from cloud!\n");
  fflush(stdout);
}

KAFU_DEST(g, "edge1")
KAFU_EXPORT(g)
void g() {
  printf("Hello, from edge\n");
  fflush(stdout);
}
