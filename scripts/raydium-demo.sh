#!/usr/bin/env bash
# Runs anchor-surface on real IDL history published by Raydium in
# https://github.com/raydium-io/raydium-idl and saves the output in demo/.
# The IDL files are downloaded, not copied into this repository.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

git clone --quiet https://github.com/raydium-io/raydium-idl.git "$WORK/raydium-idl"
cd "$WORK/raydium-idl"

# Pinned commits, oldest first.
CLMM_OLD=03850ae09eb07f7386b6ada51587b39714b87808   # 2024-08-09, legacy format
CLMM_MID=c8507c78618eda1de96ff5e43bd29daefa7e9307   # 2024-12-17, legacy format
ANCHOR31=6123104304ebcb42be175cc297a2c221ac96bb96   # 2025-06-10, regenerated with Anchor 0.31
LATEST=e7e0c96fe77bcf6a020b84a44c47a722aac8e359     # 2026-05-18

git show "$CLMM_OLD:raydium_clmm/amm_v3.json"        > "$WORK/clmm_2024-08.json"
git show "$CLMM_MID:raydium_clmm/amm_v3.json"        > "$WORK/clmm_2024-12.json"
git show "$ANCHOR31:raydium_clmm/amm_v3.json"        > "$WORK/clmm_2025-06.json"
git show "$LATEST:raydium_clmm/raydium_clmm.json"    > "$WORK/clmm_2026-05.json"
git show "$ANCHOR31:raydium_cpmm/raydium_cp_swap.json" > "$WORK/cpmm_2025-06.json"
git show "$LATEST:raydium_cpmm/raydium_cp_swap.json"   > "$WORK/cpmm_2026-05.json"

cd "$WORK"
cargo build --quiet --release --manifest-path "$ROOT/Cargo.toml"
BIN="$ROOT/target/release/anchor-surface"
mkdir -p "$ROOT/demo"

run() {
  local name="$1" old="$2" new="$3"
  "$BIN" "$old" "$new" --fail-on never > "$ROOT/demo/$name.txt"
  echo "demo/$name.txt: $(tail -n 2 "$ROOT/demo/$name.txt" | head -n 1)"
}

run raydium_cpmm_2025-06_to_2026-05 cpmm_2025-06.json cpmm_2026-05.json
run raydium_clmm_2025-06_to_2026-05 clmm_2025-06.json clmm_2026-05.json
run raydium_clmm_2024-08_to_2024-12 clmm_2024-08.json clmm_2024-12.json
run raydium_clmm_2024-12_to_2025-06 clmm_2024-12.json clmm_2025-06.json
