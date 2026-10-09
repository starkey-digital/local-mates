#!/usr/bin/env bash
# Collects the release exe plus a checksum-verified wintun.dll into dist/.
set -euo pipefail

WINTUN_SHA256=07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51

curl -sSfLo wintun.zip https://www.wintun.net/builds/wintun-0.14.1.zip
echo "$WINTUN_SHA256  wintun.zip" | sha256sum -c -
unzip -q wintun.zip
mkdir -p dist
cp target/release/local-mates.exe wintun/bin/amd64/wintun.dll dist/
