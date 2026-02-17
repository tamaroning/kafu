## kafu.h

This header file provides C/C++ macros for executing WebAssembly programs across multiple nodes.

## `KAFU_EXPORT(<func-name>)`

This macro exports the annotated function under the name `<func-name>`.  
The exported name needs to match the actual C/C++ function symbol.  
If a different name is used, the same `<func-name>` must be specified in `KAFU_DEST`.

**Notes**
- Functions annotated with this macro must not be declared as `static`.
- This macro expands to a function attribute and must be placed immediately before the function declaration (prototype). If you don't have a separate declaration, place it immediately before the function definition.
- Multiple distinct functions must not specify the same `<func-name>`.

---

## `KAFU_DEST(<func-name>, "<node-name>")`

Specifies that execution of the function identified by `<func-name>` is dynamically switched to the node `<node-name>`.

**Notes**
- `<func-name>` must match the name specified in `KAFU_EXPORT`.

**Example**  
Switch execution to node `edge1` when calling function `f`:

```c
KAFU_EXPORT(f)
void f() {
    printf("Hello, from edge1!\n");
    fflush(stdout);
}

KAFU_DEST(f, "edge1")
```

If `f` has a declaration, it must be attached with the `KAFU_EXPORT` attribute:

```c
// Declaration
KAFU_EXPORT(f)
void f(void);

// Definition
void f(void) {
    printf("Hello, from edge1!\n");
    fflush(stdout);
}

KAFU_DEST(f, "edge1")
```
