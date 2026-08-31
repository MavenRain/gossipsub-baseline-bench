# Differential mode

`run-differential.sh` runs the harness twice with identical flags and
seed: once against the pinned baseline rev (`ee8bf12e`, gossipsub
`0.50.0-unreleased`) and once against a local rust-libp2p tree.  It
prints both summaries and the per-quantile latency ratio.

## Method

- The baseline side builds in this repo, unmodified.
- The candidate side builds in a machine-managed git worktree of this
  repo (`../gossipsub-baseline-bench-candidate` by default).  A
  generated `.cargo/config.toml` there applies a `[patch]` that swaps
  all five rust-libp2p git dependencies (`libp2p-core`, `libp2p-swarm`,
  `libp2p-gossipsub`, `libp2p-plaintext`, `libp2p-yamux`) to path
  dependencies on the local tree.  All five swap together so both
  sides use one consistent crate graph each; a partial patch would mix
  two copies of `libp2p-core` and fail to compile.
- The worktree isolates the re-resolved `Cargo.lock` and the candidate
  `target/` directory.  The script force-checks-out the worktree to
  this repo's HEAD on every run, so do not hand-edit the worktree.
- `results-diff/candidate/tree.txt` discloses the candidate directory,
  branch, revision, and dirty-file count.  The candidate run also sets
  `BENCH_LIBP2P_REV`, so the `libp2p_rev` field in the candidate
  `summary.json` discloses the candidate revision, not the pinned
  baseline rev.

## Usage

Smoke run (minutes, small payloads):

```
./run-differential.sh --nodes=10 --message-bytes=1000000 --messages=3 \
  --warmup-secs=5 --settle-secs=10 --seed=42
```

Full run (the README defaults at 5 MB):

```
./run-differential.sh --nodes=30 --message-bytes=5000000 --messages=10 \
  --latency-ms=50 --bandwidth-mbps=50 --seed=42
```

Point at a different candidate tree or branch:

```
RUST_LIBP2P_DIR=$HOME/Documents/rust-libp2p ./run-differential.sh ...
```

Do not pass `--out-dir`; the script sets it per side.

## Expectations

- Candidate = the gossipsub v1.4 step-1 branch (wire types plus
  capability flag, rust-libp2p PR #6599): parity with baseline within
  run-to-run noise.  A step-1 run is a harness validity check, not a
  performance claim.
- Candidate = the step-2 fragmentation branch (planned): the measured
  delta at 1-5 MB becomes evidence for the step-2 PR body.

## Caveats

- Both sides re-emulate in real time; run sequentially on an otherwise
  idle machine and compare quantiles, not single observations.
- The candidate `Cargo.lock` re-resolves against the local tree, so
  transitive crate versions can differ from the pinned baseline lock.
  The disclosure in `tree.txt` plus the pinned baseline rev keeps the
  comparison attributable.
- Two release builds of the libp2p stack need several GB of disk
  across the two target directories.
