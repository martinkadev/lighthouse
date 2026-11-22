#!/usr/bin/env bash
# Start an EIP-8321 devnet with Kurtosis.
#
# Usage: ./start_interop.sh [-n network_params.yaml] [-e enclave] [-b true|false] [-l]
#
#   -l  vendor the Kurtosis packages ethereum-package imports (redis, postgres, prometheus,
#       db-adminer) next to it and `replace` them locally. Needed where the Kurtosis engine
#       container cannot reach GitHub itself (e.g. behind an HTTP proxy).
#
# 1. Checks out ethereum-package at the revision used by the Prysm EIP-8321 team and applies
#    ethereum-package.patch (adds `eip8321_fork_epoch`/`eip8321_fork_version` to the generated
#    config.yaml and lets the Prysm VC reach the gRPC endpoints it uses for EIP-8321 duties).
# 2. Builds the Lighthouse image with the minimal preset (`spec-minimal`).
# 3. Runs the enclave from the local, patched package.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
ROOT_DIR="$SCRIPT_DIR/../../.."
ENCLAVE_NAME=eip8321-interop
NETWORK_PARAMS_FILE="$SCRIPT_DIR/network_params_interop.yaml"
BUILD_IMAGE=true
LOCAL_DEPS=false
# Same revision as Humantvis/prysm testing/devnet/eip8321/Prepare-Package.ps1.
ETHEREUM_PKG_REV=5795b77e65235ad223d4d3b413b2739190bf60cc
ETHEREUM_PKG_DIR="${ETHEREUM_PKG_DIR:-$SCRIPT_DIR/.ethereum-package}"

while getopts "n:e:b:lh" flag; do
  case "${flag}" in
    n) NETWORK_PARAMS_FILE=${OPTARG} ;;
    e) ENCLAVE_NAME=${OPTARG} ;;
    b) BUILD_IMAGE=${OPTARG} ;;
    l) LOCAL_DEPS=true ;;
    h)
      sed -n '2,16p' "$0"
      exit 0
      ;;
    *) exit 1 ;;
  esac
done

for cmd in docker kurtosis git yq; do
  command -v "$cmd" &>/dev/null || { echo "missing dependency: $cmd" >&2; exit 1; }
done

# --- ethereum-package --------------------------------------------------------------------
if [ ! -d "$ETHEREUM_PKG_DIR/.git" ]; then
  git clone https://github.com/ethpandaops/ethereum-package "$ETHEREUM_PKG_DIR"
fi
git -C "$ETHEREUM_PKG_DIR" fetch --quiet origin "$ETHEREUM_PKG_REV" || true
git -C "$ETHEREUM_PKG_DIR" checkout --quiet --force "$ETHEREUM_PKG_REV"
git -C "$ETHEREUM_PKG_DIR" reset --quiet --hard "$ETHEREUM_PKG_REV"
git -C "$ETHEREUM_PKG_DIR" apply "$SCRIPT_DIR/ethereum-package.patch"
echo "ethereum-package @ $ETHEREUM_PKG_REV patched for EIP-8321"

if [ "$LOCAL_DEPS" = true ]; then
  DEPS_DIR="$ETHEREUM_PKG_DIR/../.kurtosis-deps"
  mkdir -p "$DEPS_DIR"
  echo "replace:" >> "$ETHEREUM_PKG_DIR/kurtosis.yml"
  for dep in kurtosis-tech/redis-package kurtosis-tech/postgres-package \
             kurtosis-tech/prometheus-package bharath-123/db-adminer-package; do
    name=${dep#*/}
    [ -d "$DEPS_DIR/$name/.git" ] || git clone --quiet --depth 1 "https://github.com/$dep" "$DEPS_DIR/$name"
    echo "  github.com/$dep: ../.kurtosis-deps/$name" >> "$ETHEREUM_PKG_DIR/kurtosis.yml"
  done
  echo "Kurtosis package dependencies vendored in $DEPS_DIR"
fi

# --- images ------------------------------------------------------------------------------
# Lighthouse: a plain local tag (no `/`, e.g. `lighthouse:eip8321-minimal`) is built from this
# checkout unless `-b false`; any `<owner>/<name>` image (e.g. the published
# `ghcr.io/martinkadev/lighthouse:eip8321`) is pulled instead.
LH_IMAGE=$(yq eval '[.participants[] | select(.cl_type == "lighthouse") | .cl_image][0]' "$NETWORK_PARAMS_FILE")
if [ "$LH_IMAGE" != "null" ] && [ "$BUILD_IMAGE" = true ] && [[ "$LH_IMAGE" != */* ]]; then
  echo "Building $LH_IMAGE (portable,spec-minimal)"
  docker build --build-arg FEATURES=portable,spec-minimal -f "$ROOT_DIR/Dockerfile" -t "$LH_IMAGE" "$ROOT_DIR"
fi

# Every other image must exist locally or be pullable.
for image in $(yq eval '.participants[] | (.cl_image, .vc_image, .el_image) | select(. != null)' "$NETWORK_PARAMS_FILE" | sort -u); do
  if [[ "$image" == *"<"* ]]; then
    echo "Fill in the <...> placeholders in $NETWORK_PARAMS_FILE first ($image)." >&2
    exit 1
  fi
  docker image inspect "$image" &>/dev/null && continue
  echo "Pulling $image"
  if ! docker pull "$image"; then
    echo "Image $image is neither available locally nor pullable." >&2
    echo "Ask the team that owns it for the image (docker save/load or a registry), see README.md." >&2
    exit 1
  fi
done

# --- run ---------------------------------------------------------------------------------
kurtosis enclave rm -f "$ENCLAVE_NAME" 2>/dev/null || true
kurtosis run --enclave "$ENCLAVE_NAME" "$ETHEREUM_PKG_DIR" --args-file "$NETWORK_PARAMS_FILE"

echo
echo "Started enclave $ENCLAVE_NAME. After epoch ~10 run:"
echo "  python3 $SCRIPT_DIR/check_interop.py --enclave $ENCLAVE_NAME --params $NETWORK_PARAMS_FILE"
