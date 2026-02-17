#include "kafu.h"
#include "spectest.h"

KAFU_DEST(f, "cloud1")
KAFU_EXPORT(f)
void f();

KAFU_DEST(g, "edge1")
KAFU_EXPORT(g)
void g();

int main() {
  f();
}

void f() {
  spectest_print_i32(1);
  g();
  spectest_print_i32(3);
}

void g() {
  spectest_print_i32(2);
}
