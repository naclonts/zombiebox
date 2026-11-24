#!/usr/bin/env sh
# Exercise the public header and exported symbols using a real native caller.
set -eu
cd "$(dirname "$0")/.."
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT
cargo build --locked -p zombiebox-ffi --target-dir "$build_dir"
lib_dir="$build_dir/debug"
"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -Icrates/zombiebox-ffi/include crates/zombiebox-ffi/tests/host.c \
    -L"$lib_dir" -Wl,-rpath,"$lib_dir" -lzombiebox_ffi -o "$build_dir/host"
"$build_dir/host"
