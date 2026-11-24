#!/usr/bin/env sh
set -eu

android_dir=$(CDPATH='' cd -- "$(dirname "$0")" && pwd)
project_dir=$(CDPATH='' cd -- "$android_dir/../../.." && pwd)
output_dir=$1
shift

sdk_dir=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}
ndk_dir=${ANDROID_NDK_HOME:-${sdk_dir:+$sdk_dir/ndk/27.1.12297006}}
if [ ! -d "$ndk_dir/toolchains/llvm/prebuilt" ]; then
    echo 'Set ANDROID_HOME to an Android SDK with NDK 27.1.12297006 installed.' >&2
    exit 1
fi

case "$(uname -s)" in
    Linux) host=linux-x86_64 ;;
    Darwin) host=darwin-x86_64 ;;
    *) echo 'Building Rust for Android requires a Linux or macOS host.' >&2; exit 1 ;;
esac
clang_dir="$ndk_dir/toolchains/llvm/prebuilt/$host/bin"
target_dir="$project_dir/target/android"
cd "$project_dir"

for abi in "$@"; do
    case "$abi" in
        arm64-v8a) target=aarch64-linux-android ;;
        x86_64) target=x86_64-linux-android ;;
        *) echo "Unsupported Android ABI: $abi" >&2; exit 1 ;;
    esac
    linker="$clang_dir/${target}24-clang"
    linker_env="CARGO_TARGET_$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')_LINKER"
    env "$linker_env=$linker" cargo build --locked --release -p zombiebox-ffi \
        --target "$target" --target-dir "$target_dir"

    mkdir -p "$output_dir/$abi"
    "$linker" -std=c11 -O2 -fPIC -shared -Wall -Wextra -Werror \
        -I "$project_dir/crates/zombiebox-ffi/include" \
        "$android_dir/zombiebox_jni.c" "$target_dir/$target/release/libzombiebox_ffi.a" \
        -landroid -ldl -ljnigraphics -llog -lm \
        -Wl,--exclude-libs,ALL -Wl,-z,max-page-size=16384 \
        -Wl,-soname,libzombiebox_jni.so -o "$output_dir/$abi/libzombiebox_jni.so"
done
