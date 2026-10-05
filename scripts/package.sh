#!/usr/bin/env bash
# Build a release tarball for x86_64 Linux.
#
# The binary is built inside Ubuntu 22.04 so it needs only glibc 2.35, not the
# host's: it runs on Ubuntu 22.04+, Debian 12+, Fedora 36+ and Arch.
# Output: dist/khz-player-<version>-x86_64-linux.tar.gz and its .sha256.
set -euo pipefail

cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
rust=$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
name="khz-player-$version-x86_64-linux"
image="khz-player-build:$rust"

docker build -t "$image" - <<EOF
FROM ubuntu:22.04
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential pkg-config libasound2-dev curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:\$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain $rust \
 && chmod -R a+rwX /usr/local/rustup /usr/local/cargo
EOF

# Separate target dir: build scripts compiled against the host glibc would not
# run in the container, and the other way round.
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/src" -w /src \
    -e CARGO_HOME=/src/target/package/cargo-home \
    -e CARGO_TARGET_DIR=/src/target/package \
    "$image" cargo build --release --locked

stage="dist/$name"
rm -rf "$stage" && mkdir -p "$stage"
install -Dm755 target/package/release/khz-player "$stage/bin/khz-player"
install -Dm644 khz-player.desktop "$stage/share/applications/khz-player.desktop"
hicolor="$stage/share/icons/hicolor"
for s in 16 24 32; do
    install -Dm644 "assets/icons/dark-small/png/khz-icon-dark-small-$s.png" "$hicolor/${s}x${s}/apps/khz-player.png"
done
for s in 48 64 128 256 512; do
    install -Dm644 "assets/icons/dark/png/khz-icon-dark-$s.png" "$hicolor/${s}x${s}/apps/khz-player.png"
done
install -Dm644 assets/icons/dark-small/khz-icon-dark-small.svg "$hicolor/scalable/apps/khz-player.svg"
install -Dm755 scripts/install.sh "$stage/install.sh"
install -Dm644 LICENSE README.md CHANGELOG.md -t "$stage"

tar -C dist -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "dist/$name.tar.gz"
