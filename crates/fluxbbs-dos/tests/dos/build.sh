#!/bin/sh
# Rebuild the test programs (needs nasm); the .com files are committed.
cd "$(dirname "$0")" && for f in *.asm; do nasm -f bin -o "${f%.asm}.com" "$f" || exit 1; done
