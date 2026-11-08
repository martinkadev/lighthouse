#!/bin/bash
set -euo pipefail

toolchain=${LOCKBUD_TOOLCHAIN:-nightly-2026-02-07}
sysroot=$(rustc +"$toolchain" --print sysroot)
export LD_LIBRARY_PATH="$sysroot/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# Analyze Lighthouse libraries, where the application's synchronization lives.
# Keep registry dependencies out of the detector while still compiling them.
libraries=$(cargo +"$toolchain" metadata --locked --no-deps --format-version 1 |
    jq -er '[.packages[].targets[] | select(.kind | index("lib")) | .name] | unique | join(",")')
test -n "$libraries"

exec cargo +"$toolchain" lockbud -k deadlock -l "$libraries" --exit-code-on-bug -- --locked --workspace --lib
