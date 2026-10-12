#!/usr/bin/env bash
# Build a manylinux wheel (and the sdist) for mapli.
#
# This must run inside a quay.io/pypa/manylinux_2_34_<arch> container (x86_64 or aarch64), from the
# repository root. The wheel's platform tag follows the container's architecture. CI runs it as a
# container job; locally, run it with docker:
#
#   docker run --rm -v "$PWD":/io -w /io \
#       -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
#       -e CARGO_TARGET_DIR=/build/target -v mapli-manylinux-build:/build \
#       quay.io/pypa/manylinux_2_34_x86_64 scripts/build-manylinux-wheel.sh
#
# For an aarch64 wheel, use the quay.io/pypa/manylinux_2_34_aarch64 image instead (and a separate
# build volume). On an x86_64 host this needs QEMU emulation (`--platform linux/arm64`) and is slow.
#
# Setting CARGO_TARGET_DIR to a docker volume keeps the container's build out of the host's
# `target/` dir and makes reruns incremental.
#
# Usage: build-manylinux-wheel.sh [deps|build|all] (default: all)
#   deps   install system packages, Rust and maturin
#   build  build the wheel and sdist into $OUT_DIR (default: dist). Set BUILD_SDIST=0 to skip the
#          sdist, e.g. when building the same release for several architectures.
set -euxo pipefail

PLAT=manylinux_2_34_$(uname -m)
PY=/opt/python/cp314-cp314/bin/python
OUT_DIR=${OUT_DIR:-dist}
BUILD_SDIST=${BUILD_SDIST:-1}
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"

install_deps() {
    dnf install -y -q epel-release
    dnf config-manager --set-enabled crb
    dnf install -y -q \
        libcurl-devel libuv-devel libicu-devel libpng-devel libjpeg-turbo-devel libwebp-devel \
        fontconfig-devel mesa-libEGL-devel mesa-libGL-devel glslang glslang-devel zlib-devel \
        openssl-devel ccache

    if ! command -v cargo >/dev/null && [ ! -x "$CARGO_BIN/cargo" ]; then
        curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    fi

    "$PY" -m pip install -q maturin
}

build() {
    export PATH="$CARGO_BIN:$PATH"
    local raw_dir
    raw_dir=$(mktemp -d)

    "$PY" -m maturin build --release --locked --compatibility "$PLAT" --auditwheel skip \
        -i "$PY" -o "$raw_dir"

    # Bundle the shared libraries the wheel links against, except:
    # - EGL/GL: libglvnd's dispatchers must load the host's own graphics driver.
    # - libcurl: a bundled RHEL build looks for CA certificates at /etc/pki/tls/..., which other
    #   distros don't have, so every HTTPS request fails. The system libcurl knows its own
    #   distro's CA paths. The extension uses unversioned curl symbols, so any libcurl.so.4 works.
    auditwheel repair --plat "$PLAT" \
        --exclude libcurl.so.4 \
        --exclude libEGL.so.1 --exclude libGL.so.1 --exclude libGLX.so.0 --exclude libOpenGL.so.0 \
        -w "$OUT_DIR" "$raw_dir"/mapli-*.whl
    auditwheel show "$OUT_DIR"/mapli-*.whl

    if [ "$BUILD_SDIST" = "1" ]; then
        "$PY" -m maturin sdist -o "$OUT_DIR"
    fi

    if [ -n "${HOST_UID:-}" ]; then
        chown -R "$HOST_UID:${HOST_GID:-$HOST_UID}" "$OUT_DIR"
    fi
}

case "${1:-all}" in
    deps) install_deps ;;
    build) build ;;
    all) install_deps && build ;;
    *) echo "usage: $0 [deps|build|all]" >&2; exit 2 ;;
esac
