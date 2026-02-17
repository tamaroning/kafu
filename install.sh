#!/bin/sh
# shellcheck shell=dash
# Kafu SDK installer
#
# Usage:
#   curl -fsSL <URL>/install.sh | sh
#   sh install.sh --help

set -u

KAFU_SDK_VERSION_DEFAULT="0.1.0"
WASI_SDK_VERSION_DEFAULT="25.0"
KAFU_WASM_OPT_VERSION_DEFAULT="snapify-0.1.0"

usage() {
    cat <<'EOF'
Kafu SDK installer

Usage:
  install.sh [OPTIONS]

Options:
      --version <VERSION>     SDK version to install (default: 0.1.0)
      --install-dir <DIR>     Install directory (default: $HOME/kafu-sdk-<version>)
      --tarball-url <URL>     Use an explicit tarball URL (skip arch detection)
      --wasi-sdk-version <V>  WASI SDK version (default: 25.0)
      --wasi-sdk-dir <DIR>    WASI SDK install directory (default: <install-dir>/wasi-sdk-<version>)
      --no-wasi-sdk           Do not install WASI SDK
      --wasm-opt-version <V>  wasm-opt (snapify) version tag (default: snapify-0.1.0)
      --no-wasm-opt           Do not install wasm-opt (snapify)
      --no-modify-path        Do not append env vars to your shell profile
      --force                 Remove existing install dir and reinstall
  -h, --help                  Show help

Example:
  sh install.sh --version 0.1.0
EOF
}

__print() {
    # Follow rustup-init.sh style: print to stderr
    printf '%s: %s\n' "$1" "$2" >&2
}

say() { __print "info" "$1"; }
warn() { __print "warn" "$1"; }
err() { __print "error" "$1"; }

check_cmd() {
    command -v "$1" >/dev/null 2>&1
}

need_cmd() {
    if ! check_cmd "$1"; then
        err "need '$1' (command not found)"
        exit 1
    fi
}

ensure() {
    if ! "$@"; then
        err "command failed: $*"
        exit 1
    fi
}

downloader() {
    # We don't replicate all rustup-init.sh TLS capability detection,
    # but we keep the curl/wget auto-selection behavior.
    if [ "$1" = --check ]; then
        if check_cmd curl; then
            return 0
        fi
        if check_cmd wget; then
            return 0
        fi
        err "need 'curl' or 'wget' (command not found)"
        exit 1
    fi

    _url=$1
    _out=$2

    if check_cmd curl; then
        # -f: fail on HTTP errors, -S: show errors, -L: follow redirects
        ensure curl -fSL "$_url" -o "$_out"
        return 0
    fi
    if check_cmd wget; then
        ensure wget "$_url" -O "$_out"
        return 0
    fi

    err "unknown downloader"
    exit 1
}

get_arch() {
    need_cmd uname
    _os=$(uname -s)
    _cpu=$(uname -m)

    if [ "$_os" != "Linux" ]; then
        err "Unsupported OS: $_os (Linux only)"
        exit 1
    fi

    case "$_cpu" in
        x86_64|amd64)
            echo "x86_64"
            ;;
        aarch64|arm64)
            echo "aarch64"
            ;;
        *)
            err "Unsupported CPU architecture: $_cpu (x86_64/aarch64 only)"
            exit 1
            ;;
    esac
}

install_kafu_wasm_opt() {
    _install_dir=$1
    _arch=$2
    _binaryen_version=$3
    _binaryen_repo=$4
    _tmp_dir=$5

    case "$_arch" in
        x86_64|aarch64)
            _binaryen_arch="$_arch"
            ;;
        *)
            err "Unsupported CPU architecture for wasm-opt: $_arch (x86_64/aarch64 only)"
            exit 1
            ;;
    esac

    # Release tag is like "snapify-0.1.0".
    _file_version=${_binaryen_version#snapify-}
    _url="https://github.com/${_binaryen_repo}/releases/download/${_binaryen_version}/binaryen-snapify-${_file_version}-${_binaryen_arch}-linux.tar.gz"
    _tarball="${_tmp_dir}/binaryen-snapify.tar.gz"

    say "downloading wasm-opt (snapify): ${_binaryen_version}"
    ensure downloader "$_url" "$_tarball"

    say "extracting wasm-opt (snapify)"
    ensure tar -xzf "$_tarball" -C "$_tmp_dir"

    _extracted_dir=""
    for d in "$_tmp_dir"/binaryen-*; do
        if [ -d "$d" ]; then
            _extracted_dir=$d
            break
        fi
    done

    if [ -z "${_extracted_dir:-}" ]; then
        err "Could not find extracted binaryen directory in: $_tmp_dir"
        exit 1
    fi

    if [ ! -f "${_extracted_dir}/bin/wasm-opt" ]; then
        err "wasm-opt not found in: ${_extracted_dir}/bin/wasm-opt"
        exit 1
    fi

    ensure mkdir -p "$_install_dir/libexec"
    ensure cp "${_extracted_dir}/bin/wasm-opt" "$_install_dir/libexec/kafu_wasm-opt"
    ensure chmod 755 "$_install_dir/libexec/kafu_wasm-opt"
}

detect_profiles() {
    # installation.md appends to ~/.bashrc, but we support bash/zsh/others.
    #
    # We always update the primary profile file (create if missing).
    # Additionally, if other relevant profiles already exist, we also update them.
    _primary=""
    _extras=""

    if [ -n "${ZSH_VERSION-}" ]; then
        _primary="${HOME}/.zshrc"
        _extras="${HOME}/.zprofile"
    elif [ -n "${BASH_VERSION-}" ]; then
        _primary="${HOME}/.bashrc"
        _extras="${HOME}/.bash_profile"
    elif [ -n "${SHELL-}" ]; then
        case "$SHELL" in
            */zsh)
                _primary="${HOME}/.zshrc"
                _extras="${HOME}/.zprofile"
                ;;
            */bash)
                _primary="${HOME}/.bashrc"
                _extras="${HOME}/.bash_profile"
                ;;
            *)
                _primary="${HOME}/.profile"
                _extras=""
                ;;
        esac
    else
        # Common profile for POSIX sh
        _primary="${HOME}/.profile"
        _extras=""
    fi

    _profiles="$_primary"
    for f in $_extras; do
        if [ -f "$f" ] && [ "$f" != "$_primary" ]; then
            _profiles="${_profiles} $f"
        fi
    done
    printf '%s\n' "$_profiles"
}

upsert_profile_block() {
    _profile=$1
    _label=$2
    _start_marker=$3
    _end_marker=$4

    need_cmd mkdir
    need_cmd touch
    need_cmd grep
    need_cmd awk
    need_cmd mktemp
    need_cmd mv
    need_cmd rm

    ensure mkdir -p "$(dirname "$_profile")"
    ensure touch "$_profile"

    _block_file="$(mktemp)"
    cat >"$_block_file"

    _has_start=no
    _has_end=no
    if grep -Fq "$_start_marker" "$_profile" 2>/dev/null; then
        _has_start=yes
    fi
    if grep -Fq "$_end_marker" "$_profile" 2>/dev/null; then
        _has_end=yes
    fi

    if [ "$_has_start" = "yes" ] && [ "$_has_end" = "yes" ]; then
        say "Profile already contains ${_label} block; updating: $_profile"

        _out="$(mktemp)"
        # Replace the block in-place (keep everything else)
        awk -v start="$_start_marker" -v end="$_end_marker" -v blockfile="$_block_file" '
            function print_block(  line) {
                while ((getline line < blockfile) > 0) print line
                close(blockfile)
            }
            BEGIN { inblock=0; done=0 }
            {
                if (index($0, start) > 0) {
                    inblock=1
                    if (!done) { print_block(); done=1 }
                    next
                }
                if (inblock) {
                    if (index($0, end) > 0) inblock=0
                    next
                }
                print
            }
            END {
                if (!done) print_block()
            }
        ' "$_profile" >"$_out"
        ensure mv "$_out" "$_profile"
    else
        if [ "$_has_start" = "yes" ] && [ "$_has_end" = "no" ]; then
            warn "Profile has '${_start_marker}' but missing '${_end_marker}'; appending new ${_label} block: $_profile"
        else
            say "Adding ${_label} block to profile: $_profile"
        fi
        # Safe append (don’t try to surgically replace malformed blocks)
        ensure sh -c 'cat "$1" >>"$2"' sh "$_block_file" "$_profile"
    fi

    ensure rm -f "$_block_file"
}

delete_profile_block_if_present() {
    _profile=$1
    _start_marker=$2
    _end_marker=$3

    need_cmd grep
    need_cmd awk
    need_cmd mktemp
    need_cmd mv

    if ! grep -Fq "$_start_marker" "$_profile" 2>/dev/null; then
        return 0
    fi
    if ! grep -Fq "$_end_marker" "$_profile" 2>/dev/null; then
        return 0
    fi

    warn "Removing legacy block from profile: $_profile"
    _out="$(mktemp)"
    awk -v start="$_start_marker" -v end="$_end_marker" '
        BEGIN { inblock=0 }
        {
            if (!inblock && index($0, start) > 0) { inblock=1; next }
            if (inblock) {
                if (index($0, end) > 0) { inblock=0 }
                next
            }
            print
        }
    ' "$_profile" >"$_out"
    ensure mv "$_out" "$_profile"
}

append_env() {
    _profile=$1
    _install_dir=$2
    _wasi_dir=${3-}

    need_cmd mkdir
    need_cmd touch

    ensure mkdir -p "$(dirname "$_profile")"
    ensure touch "$_profile"

    {
        printf 'export KAFU_SDK_PATH="%s"\n' "$_install_dir"
        printf '%s\n' 'export PATH="$KAFU_SDK_PATH/bin:$PATH"'
        if [ -n "${_wasi_dir:-}" ]; then
            printf 'export WASI_SDK_PATH="%s"\n' "$_wasi_dir"
        fi
    } >>"$_profile"

    # Legacy cleanup: WASI SDK used to be a separate block.
    delete_profile_block_if_present \
        "$_profile" \
        "# >>> wasi sdk install >>>" \
        "# <<< wasi sdk install <<<"
}

is_dir_nonempty() {
    # POSIX: directory non-empty check (excluding . and ..)
    _d=$1
    if [ ! -d "$_d" ]; then
        return 1
    fi
    # shellcheck disable=SC2012
    if ls -A "$_d" >/dev/null 2>&1 && [ "$(ls -A "$_d" 2>/dev/null | wc -l | tr -d ' ')" != "0" ]; then
        return 0
    fi
    return 1
}

main() {
    downloader --check
    need_cmd mktemp
    need_cmd mkdir
    need_cmd rm
    need_cmd tar

    _no_modify_path=no
    _force=no
    _no_wasi_sdk=no
    _no_wasm_opt=no

    _version="$KAFU_SDK_VERSION_DEFAULT"
    _install_dir=""
    _tarball_url=""

    _wasi_version="$WASI_SDK_VERSION_DEFAULT"
    _wasi_install_dir=""
    _wasm_opt_version="$KAFU_WASM_OPT_VERSION_DEFAULT"

    while [ $# -gt 0 ]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            --no-modify-path)
                _no_modify_path=yes
                ;;
            --force)
                _force=yes
                ;;
            --version)
                shift
                _version=${1-}
                if [ -z "${_version:-}" ]; then
                    err "--version requires a value"
                    exit 1
                fi
                ;;
            --install-dir)
                shift
                _install_dir=${1-}
                if [ -z "${_install_dir:-}" ]; then
                    err "--install-dir requires a value"
                    exit 1
                fi
                ;;
            --tarball-url)
                shift
                _tarball_url=${1-}
                if [ -z "${_tarball_url:-}" ]; then
                    err "--tarball-url requires a value"
                    exit 1
                fi
                ;;
            --wasi-sdk-version)
                shift
                _wasi_version=${1-}
                if [ -z "${_wasi_version:-}" ]; then
                    err "--wasi-sdk-version requires a value"
                    exit 1
                fi
                ;;
            --wasi-sdk-dir)
                shift
                _wasi_install_dir=${1-}
                if [ -z "${_wasi_install_dir:-}" ]; then
                    err "--wasi-sdk-dir requires a value"
                    exit 1
                fi
                ;;
            --no-wasi-sdk)
                _no_wasi_sdk=yes
                ;;
            --wasm-opt-version)
                shift
                _wasm_opt_version=${1-}
                if [ -z "${_wasm_opt_version:-}" ]; then
                    err "--wasm-opt-version requires a value"
                    exit 1
                fi
                ;;
            --no-wasm-opt)
                _no_wasm_opt=yes
                ;;
            *)
                err "Unknown argument: $1"
                err "Help: sh install.sh --help"
                exit 1
                ;;
        esac
        shift
    done

    if [ -z "${_install_dir:-}" ]; then
        _install_dir="${HOME}/kafu-sdk-${_version}"
    fi

    if [ -z "${_wasi_install_dir:-}" ]; then
        _wasi_install_dir="${_install_dir}/wasi-sdk-${_wasi_version}"
    fi

    if [ -z "${_tarball_url:-}" ]; then
        _arch=$(get_arch)
        _safe_version=$(printf '%s' "$_version" | tr '/' '-')
        _tarball_url="https://github.com/tamaroning/kafu/releases/download/${_version}/kafu-sdk-${_safe_version}-${_arch}-linux.tar.gz"
    fi

    _install_wasi=yes
    if [ "$_no_wasi_sdk" = "yes" ]; then
        _install_wasi=no
    fi

    _install_wasm_opt=yes
    if [ "$_no_wasm_opt" = "yes" ]; then
        _install_wasm_opt=no
    fi

    if [ "$_install_wasi" = "yes" ]; then
        _arch=$(get_arch)
        case "$_arch" in
            x86_64) _wasi_arch="x86_64" ;;
            aarch64) _wasi_arch="arm64" ;;
            *) err "Unsupported CPU architecture for WASI SDK: $_arch"; exit 1 ;;
        esac
        _wasi_tag="wasi-sdk-${_wasi_version%%.*}"
        _wasi_tarball_url="https://github.com/WebAssembly/wasi-sdk/releases/download/${_wasi_tag}/wasi-sdk-${_wasi_version}-${_wasi_arch}-linux.tar.gz"
    fi

    say "Kafu SDK version: ${_version}"
    say "Install dir: ${_install_dir}"
    if [ "$_install_wasi" = "yes" ]; then
        say "WASI SDK version: ${_wasi_version}"
        say "WASI SDK dir: ${_wasi_install_dir}"
    else
        say "WASI SDK: skipped (--no-wasi-sdk)"
    fi
    if [ "$_install_wasm_opt" = "yes" ]; then
        say "wasm-opt (snapify) version: ${_wasm_opt_version}"
    else
        say "wasm-opt (snapify): skipped (--no-wasm-opt)"
    fi

    if is_dir_nonempty "$_install_dir"; then
        if [ "$_force" = "yes" ]; then
            say "Removing existing install dir and reinstalling: $_install_dir"
            ensure rm -rf "$_install_dir"
        else
            err "Install directory already exists: $_install_dir"
            err "Use --force to overwrite"
            exit 1
        fi
    fi

    if [ "$_install_wasi" = "yes" ] && is_dir_nonempty "$_wasi_install_dir"; then
        if [ "$_force" = "yes" ]; then
            say "Removing existing WASI SDK dir and reinstalling: $_wasi_install_dir"
            ensure rm -rf "$_wasi_install_dir"
        else
            warn "WASI SDK directory already exists; skipping install: $_wasi_install_dir"
            _install_wasi=no
        fi
    fi

    _tmp_dir=$(ensure mktemp -d)
    _tgz="${_tmp_dir}/kafu-sdk.tar.gz"

    say "downloading kafu sdk"
    ensure downloader "$_tarball_url" "$_tgz"

    say "extracting"
    ensure mkdir -p "$_install_dir"
    ensure tar -xzf "$_tgz" -C "$_install_dir" --strip-components=1

    if [ "$_install_wasm_opt" = "yes" ]; then
        _arch=$(get_arch)
        install_kafu_wasm_opt "$_install_dir" "$_arch" "$_wasm_opt_version" "tamaroning/binaryen" "$_tmp_dir"
    fi

    if [ "$_install_wasi" = "yes" ]; then
        _wasi_tgz="${_tmp_dir}/wasi-sdk.tar.gz"
        say "downloading wasi sdk"
        ensure downloader "$_wasi_tarball_url" "$_wasi_tgz"
        say "extracting wasi sdk"
        ensure mkdir -p "$_wasi_install_dir"
        ensure tar -xzf "$_wasi_tgz" -C "$_wasi_install_dir" --strip-components=1
    fi

    ensure rm -rf "$_tmp_dir"

    if [ "$_no_modify_path" = "no" ]; then
        _profiles=$(detect_profiles)
        say "updating profile(s): ${_profiles}"
        _wasi_dir_to_set=""
        if [ "$_install_wasi" = "yes" ]; then
            _wasi_dir_to_set="$_wasi_install_dir"
        fi
        for _profile in $_profiles; do
            append_env "$_profile" "$_install_dir" "$_wasi_dir_to_set"
        done
        _first_profile=${_profiles%% *}
        say "Done. Open a new shell, or run: . \"$_first_profile\""
    else
        say "--no-modify-path set; not modifying your shell profile"
        say "If needed, set these manually:"
        say "  export KAFU_SDK_PATH=\"$_install_dir\""
        say "  export PATH=\"\$KAFU_SDK_PATH/bin:\$PATH\""
        if [ "$_install_wasi" = "yes" ]; then
            say "  export WASI_SDK_PATH=\"$_wasi_install_dir\""
        fi
    fi

    say "Verification:"
    say "  kafu --version"
    say ""
    if [ "$_install_wasi" = "yes" ]; then
        say "WASI SDK installed:"
        say "  WASI_SDK_PATH=$_wasi_install_dir"
    else
        say "Note: To build WebAssembly services, you need WASI SDK ${WASI_SDK_VERSION_DEFAULT}."
        say "      Install it separately or rerun without --no-wasi-sdk."
    fi
}

main "$@"
