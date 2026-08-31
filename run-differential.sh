#!/usr/bin/env bash
# Differential runner: stock gossipsub at the pinned baseline rev versus a
# local rust-libp2p tree (candidate).  Same harness, same flags, same seed.
#
# Usage:
#   ./run-differential.sh [bench flags...]
# Environment:
#   RUST_LIBP2P_DIR  candidate tree      (default: ~/Documents/rust-libp2p)
#   CANDIDATE_WT     candidate worktree  (default: <bench>-candidate)
#   OUT_ROOT         results root        (default: <bench>/results-diff)
#   JOBS             build jobs          (default: 2)
#   CARGO            cargo command       (default: cargo)
# Do not pass --out-dir; the script sets it per side.
set -euo pipefail

BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_LIBP2P_DIR="${RUST_LIBP2P_DIR:-$HOME/Documents/rust-libp2p}"
CANDIDATE_WT="${CANDIDATE_WT:-${BENCH_DIR}-candidate}"
OUT_ROOT="${OUT_ROOT:-$BENCH_DIR/results-diff}"
JOBS="${JOBS:-2}"
# cargocho is a MODE-based output distiller, not argv-compatible with
# cargo; the bench run needs plain cargo and its live output.
CARGO="${CARGO:-cargo}"

if [ ! -d "$RUST_LIBP2P_DIR/protocols/gossipsub" ]; then
  echo "error: no rust-libp2p tree at $RUST_LIBP2P_DIR" >&2
  exit 1
fi

# Candidate worktree: isolates the re-resolved Cargo.lock and target dir.
# The worktree is machine-managed; local edits in it are discarded.
BENCH_HEAD="$(git -C "$BENCH_DIR" rev-parse HEAD)"
if [ ! -d "$CANDIDATE_WT" ]; then
  git -C "$BENCH_DIR" worktree add --detach "$CANDIDATE_WT" "$BENCH_HEAD"
else
  git -C "$CANDIDATE_WT" checkout --force --detach "$BENCH_HEAD"
fi

# Config-based [patch]: swap every rust-libp2p git dep to the local tree.
mkdir -p "$CANDIDATE_WT/.cargo"
cat > "$CANDIDATE_WT/.cargo/config.toml" <<EOF
[patch."https://github.com/libp2p/rust-libp2p"]
libp2p-core = { path = "$RUST_LIBP2P_DIR/core" }
libp2p-swarm = { path = "$RUST_LIBP2P_DIR/swarm" }
libp2p-gossipsub = { path = "$RUST_LIBP2P_DIR/protocols/gossipsub" }
libp2p-plaintext = { path = "$RUST_LIBP2P_DIR/transports/plaintext" }
libp2p-yamux = { path = "$RUST_LIBP2P_DIR/muxers/yamux" }
EOF

mkdir -p "$OUT_ROOT/baseline" "$OUT_ROOT/candidate"

# Disclose exactly what the candidate side was built from.
CAND_REV="$(git -C "$RUST_LIBP2P_DIR" rev-parse HEAD)"
{
  echo "rust_libp2p_dir: $RUST_LIBP2P_DIR"
  echo "branch: $(git -C "$RUST_LIBP2P_DIR" branch --show-current)"
  echo "rev: $CAND_REV"
  echo "dirty_files: $(git -C "$RUST_LIBP2P_DIR" status --porcelain | wc -l | tr -d ' ')"
} > "$OUT_ROOT/candidate/tree.txt"
cat "$OUT_ROOT/candidate/tree.txt"

echo "== baseline: build + run =="
(cd "$BENCH_DIR" && "$CARGO" run --release -j "$JOBS" -- "$@" --out-dir="$OUT_ROOT/baseline")

echo "== candidate: build + run (patched to $RUST_LIBP2P_DIR) =="
(cd "$CANDIDATE_WT" && BENCH_LIBP2P_REV="$CAND_REV" "$CARGO" run --release -j "$JOBS" -- "$@" --out-dir="$OUT_ROOT/candidate")

echo "== comparison =="
if command -v jq >/dev/null 2>&1; then
  for side in baseline candidate; do
    echo "-- $side"
    jq -c '{delivery_latency_ms, completion_ms}' "$OUT_ROOT/$side/summary.json"
  done
  jq -rn \
    --slurpfile b "$OUT_ROOT/baseline/summary.json" \
    --slurpfile c "$OUT_ROOT/candidate/summary.json" '
    ["p50","p90","p99","max"][] as $q |
    ($b[0].delivery_latency_ms[$q]) as $bv |
    ($c[0].delivery_latency_ms[$q]) as $cv |
    "delivery \($q): baseline \($bv) ms, candidate \($cv) ms, ratio \(($cv / $bv) * 100 | round / 100)"
  '
else
  echo "jq not found; summaries at $OUT_ROOT/{baseline,candidate}/summary.json"
fi
