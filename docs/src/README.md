# Introduction

## Overview

[Kafu](https://github.com/tamaroning/kafu/) is a WebAssembly-based framework designed to transparently develop and execute distributed services.  
Unlike FaaS or traditional client–server models, Kafu allows developers to focus solely on business logic, without explicitly handling service placement, communication, or orchestration.

Kafu compiles C/C++ source code into WebAssembly modules and executes them in a lightweight runtime, enabling portable and efficient distributed computing across cloud and edge environments. Through its built-in binary translator, [Snapify](development/snapify.md), Kafu instruments WebAssembly modules at compile time to support checkpoint/restore, allowing live migration of running programs between nodes without any interruption.

## Why WebAssembly?

WebAssembly offers portability, allowing the same binary to run across diverse environments including cloud and edge computing. It can be compiled to WebAssembly from various programming languages such as C, C++, Rust, and Go. The runtime is lightweight, supporting fast program startup, suspension, and resumption (checkpoint/restore). By leveraging WebAssembly, Kafu enables a transparent programming model in C/C++ while executing programs in a high-performance runtime environment.

## Features

- A C/C++-to-WebAssembly compiler
- C/C++ function attributes for dynamically switching the execution node of a service
- Tooling to deploy services as a Kubernetes cluster

## Supported Standards

- **WebAssembly 2.0**: Kafu fully supports the [WebAssembly 2.0](https://www.w3.org/TR/wasm-core-2/) specification, including features such as multi-value returns, reference types, bulk memory operations, and SIMD instructions.
- **WASI Snapshot preview1**: Kafu implements the WASI snapshot preview1 interface, providing system-level capabilities such as file I/O, environment variables, and clock access to WebAssembly modules.

## Status

Kafu is currently in an experimental stage and is not recommended for production use.
