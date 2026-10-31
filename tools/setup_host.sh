#!/usr/bin/env bash
# Get a fresh clone ready: build, and unpack the stock Iniquity install for
# the legacy tests if you have its release (original/inqa26r5.zip; it isn't
# in git). The board makes its SysOp (SysOp / SYSOP) on its first start.
#
#   tools/setup_host.sh            # from the repo root
#
# Needs: cargo (and unzip for the release). Safe to run again; it never
# touches data/ or the *.toml configuration.
set -euo pipefail
cd "$(dirname "$0")/.."

stock=original/stock
if [ ! -f "$stock/INIQUITY.DAT" ] && [ -f original/inqa26r5.zip ]; then
    echo "unpacking the stock install into $stock"
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    unzip -q original/inqa26r5.zip -d "$tmp/release"
    # newinst.exe is a self-extracting ZIP holding the install itself.
    mkdir -p "$stock"
    unzip -q -o "$tmp/release/newinst.exe" -d "$stock"
fi

# FEATURES picks the optional components (docs/BUILDING.md); everything
# by default, as the flagship board runs. FEATURES= builds the core.
FEATURES=${FEATURES-full}
echo "building${FEATURES:+ with $FEATURES}"
cargo build --release ${FEATURES:+--features "$FEATURES"}

echo
echo "ready:"
echo "  ./target/release/fluxbbs                       # SSH :2222, telnet :2323"
echo "  cargo test --features full                       # the legacy readers too, with original/stock"
echo "  log in as SysOp / SYSOP (system password SYSTEM); change both before going public"
