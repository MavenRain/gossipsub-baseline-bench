# Tuned stock gossipsub at 4-10 MB: a disclosed baseline

This report gives the first fully disclosed benchmark of tuned stock
rust-libp2p gossipsub at 4-10 MB message sizes. Every configuration knob is
listed in each run's `summary.json`. The harness, topology, link model, and
seeds are in this repository. Anyone can reproduce every number.

## Why this exists

OptimumP2P publishes RLNC-based improvement factors against "gossipsub":
2.05x mean latency at 5 MB, and 2.72x measured (7.88x claimed) tail latency.
The baseline configuration behind those numbers is not disclosed. That
matters more than it sounds:

- Stock rust-libp2p ships `max_transmit_size = 65536`. A default-configured
  node cannot carry a 5 MB message at all. Any benchmark that ran against
  defaults, or near-defaults, measured a strawman.
- Our B-grid below shows that bandwidth alone moves the stock p50 at 5 MB
  from 3.25 s to 0.98 s (3.3x). An undisclosed baseline environment can
  produce almost any ratio.

So the vendor factors are not treated as targets here. The question this
baseline answers is: what does *tuned* stock gossipsub actually do at these
sizes, and where does the time and bandwidth go?

## Setup (full disclosure in every summary.json)

- rust-libp2p pinned at git rev `ee8bf12e6d94d48518ea67773abb11625b2c4f41`
  (master 2026-08-24, gossipsub 0.50.0-unreleased; crates.io max is 0.49.5).
- 30 nodes in one process. Topology: ring plus seeded random edges, degree
  12. Mesh D=8 (D_lo 6, D_hi 12); observed steady-state mesh degree 6-9.
- Link model: `ShapedIo` on the receive path below yamux.
  `release = max(arrival, serializer_free) + len/bandwidth + latency`.
  Yamux flow-control windows back-pressure through the emulated link.
- Defaults per cell: 50 ms one-way latency, 50 Mbps per link, strict
  validation, signed messages, heartbeat 1 s, history 5/3,
  `max_transmit_size` = 2x payload, `flood_publish` off. Node 0 publishes.
- Transport: memory + plaintext + yamux with default 256 KiB stream
  windows. Crypto CPU is excluded by design and disclosed. Signing cost is
  reported separately as `publish_call_ms`.

## Results

12 runs, all complete, zero delivery shortfall. Full table:
`results/SWEEP.md`, machine-readable `results/sweep-table.jsonl`.

### Size ladder (50 ms / 50 Mbps, idontwant_on_publish off/on)

| size | idw | p50 ms | p90 ms | p99 ms | max ms | amplification |
|------|-----|--------|--------|--------|--------|---------------|
| 4 MB | off | 1478 | 2309 | 3510 | 3864 | 5.82 |
| 4 MB | on | 1512 | 2352 | 3021 | 3512 | 6.04 |
| 5 MB | off | 1954 | 2642 | 2920 | 3320 | 5.98 |
| 5 MB | on | 1816 | 2658 | 2940 | 3114 | 5.84 |
| 8 MB | off | 2848 | 3933 | 4835 | 6289 | 5.78 |
| 8 MB | on | 2817 | 3434 | 3933 | 3976 | 5.76 |
| 10 MB | off | 3484 | 4351 | 5154 | 5304 | 5.82 |
| 10 MB | on | 3594 | 4743 | 5263 | 5522 | 5.49 |

### Latency/bandwidth grid (5 MB, idw off)

| latency | bandwidth | p50 ms | p90 ms | amplification |
|---------|-----------|--------|--------|---------------|
| 25 ms | 25 Mbps | 3251 | 3385 | 6.39 |
| 25 ms | 100 Mbps | 977 | 1264 | 5.58 |
| 100 ms | 25 Mbps | 3450 | 4379 | 6.27 |
| 100 ms | 100 Mbps | 1156 | 2558 | 6.37 |

## What the numbers say

**1. Latency tracks store-and-forward serialization, not propagation
delay.** One hop must serialize the full payload before the next hop
starts: 0.8 s at 5 MB / 50 Mbps, 1.6 s at 10 MB. The p50 at each size sits
near 2 to 2.2 hop-serializations plus link latency. In the grid, a 4x
bandwidth change moves p50 by ~2.3 s while a 4x latency change moves it by
~0.2 s. This is the mechanism punchlist items 2 (fragmentation with
pipelined relay) and 4 (v1.4 PREAMBLE) attack, and it is the dominant term.

**2. Amplification is stable at 5.5-6.4x everywhere.** Each delivered
payload costs ~6 payloads of wire traffic, shaped by mesh degree (observed
6-9) and eager push. IDONTWANT (v1.2) already runs in every cell; this is
the residual it does not remove at multi-second transmission times. This is
the A3 (duplicates/bandwidth) budget that items 3, 4, and 5 target.

**3. `idontwant_on_publish` (ships default OFF) is a tail and wire knob,
not a median knob.** Median deltas are noise and sign-flip (+34 ms at 4 MB,
-138 ms at 5 MB). The consistent effects: tail compression (8 MB max drops
from 6.29 s to 3.98 s) and wire reduction at the largest size (10 MB
amplification 5.82 to 5.49). At 4 MB it slightly *raises* wire cost: the
payload clears the mesh before cancellations land.

**4. Nothing breaks at these sizes once `max_transmit_size` is raised.**
All 240 expected deliveries across the ladder arrived, including 10 MB
payloads through an 8 MiB shaper queue (back-pressure, not drops). The
"stock gossipsub cannot do large messages" framing conflates a config
default with a protocol limit.

## Reading the vendor numbers against this baseline

- A 2.05x mean-latency improvement at 5 MB, applied to our tuned-stock p50
  of 1954 ms (50 Mbps / 50 ms), predicts ~953 ms. Our stock run at 100 Mbps
  reaches 977 ms with no protocol change. The claimed factor is inside the
  envelope that baseline environment choices alone can produce. Without the
  vendor's baseline config, the factor is not interpretable.
- The *mechanisms* behind RLNC's advantage remain real and are visible
  here: the serialization ladder (finding 1) is exactly what cut-through
  and fragmentation remove, and the ~6x amplification (finding 2) is the
  duplicate budget. The honest statement is: stock gossipsub pays ~2 full
  serializations of median latency and ~6x wire cost at 5 MB; fragmentation
  with pipelined relay plus PREAMBLE can recover much of both without
  RLNC; relay recoding and k-of-n completion remain RLNC-only.

## Threats to validity

- Real-time in-process emulation: 30 swarms share one machine, so CPU
  contention adds jitter. Wall clocks per run were 22-42 s with no sign of
  starvation, but individual samples carry noise; single seed per cell.
- Plaintext auth: crypto CPU (except signing, reported separately at
  3.6-8.6 ms per publish) is excluded. Real deployments pay Noise/TLS.
- Yamux default 256 KiB stream windows cap per-hop throughput at roughly
  window/RTT. At 50 ms one-way latency that is ~20 Mbps effective. This is
  stock behavior and is deliberately not tuned away; a window-size
  sensitivity sweep is a natural follow-up.
- Message rate is low (one publisher, sequential-ish publishes). Congestion
  from concurrent publishers is out of scope for this baseline.

## Reproduce

```
cargo run --release -- --nodes=30 --message-bytes=5000000 --messages=10 \
  --latency-ms=50 --bandwidth-mbps=50 --idontwant-on-publish=false \
  --warmup-secs=12 --settle-secs=45 --seed=42 --out-dir=results/a-5mb-idw0
```

Each `results/<run>/summary.json` embeds the complete configuration,
including the libp2p git rev.
