#!/usr/bin/env bash
# Get a fresh clone ready: unpack the stock Iniquity install for the tests,
# give the board a starting database if it has none, and build.
#
#   tools/setup_host.sh            # from the repo root
#
# Needs: cargo, unzip. Safe to run again; it never touches an existing
# data/fluxbbs.db (or a pre-rename iniquity.db) or the *.toml configuration.
set -euo pipefail
cd "$(dirname "$0")/.."

stock=original/stock
if [ ! -f "$stock/INIQUITY.DAT" ]; then
    echo "unpacking the stock install into $stock"
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    unzip -q original/inqa26r5.zip -d "$tmp/release"
    # newinst.exe is a self-extracting ZIP holding the install itself.
    mkdir -p "$stock"
    unzip -q -o "$tmp/release/newinst.exe" -d "$stock"
fi

echo "building"
cargo build --release

if [ ! -f data/fluxbbs.db ] && [ ! -f data/iniquity.db ]; then
    # Import the stock install somewhere else and keep only its database
    # (users, including SysOp, statistics, the BBS list), so the committed
    # configuration stays as it is.
    echo "creating data/fluxbbs.db from the stock install"
    tmp_import=$(mktemp -d)
    ./target/release/fluxbbs import "$stock" "$tmp_import" > /dev/null
    mkdir -p data
    cp "$tmp_import/data/fluxbbs.db" data/fluxbbs.db
    rm -rf "$tmp_import"
fi

echo
echo "ready:"
echo "  ./target/release/fluxbbs                       # SSH :2222, telnet :2323"
echo "  cargo test                                      # includes the legacy readers"
echo "  log in as SysOp / SYSOP (system password SYSTEM); change both before going public"
