# Build the project
[working-directory: '.']
build:
	cargo build

# Run all tests
[working-directory: '.']
test:
	cargo test --workspace --all-targets --locked

# Run clippy
[working-directory: '.']
clippy:
	cargo clippy --workspace --all-targets --locked -- -D warnings

# Format (Rust + C/C++)
[working-directory: '.']
fmt:
	cargo fmt --all
	just fmt-c

# Check formatting (Rust + C/C++)
[working-directory: '.']
fmt-check:
	cargo fmt --all --check
	just fmt-c-check

# Format C/C++ sources (tracked files only)
[working-directory: '.']
fmt-c:
	bash -lc 'set -euo pipefail; mapfile -t files < <(git ls-files -- "*.c" "*.h" "*.cc" "*.cpp" "*.cxx" "*.hpp" "*.hxx"); if [ "${#files[@]}" -eq 0 ]; then exit 0; fi; clang-format -i "${files[@]}"'

# Check C/C++ formatting (no modifications)
[working-directory: '.']
fmt-c-check:
	bash -lc 'set -euo pipefail; mapfile -t files < <(git ls-files -- "*.c" "*.h" "*.cc" "*.cpp" "*.cxx" "*.hpp" "*.hxx"); if [ "${#files[@]}" -eq 0 ]; then exit 0; fi; clang-format --dry-run --Werror "${files[@]}"'
