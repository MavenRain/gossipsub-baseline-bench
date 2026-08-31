# gossipsub-baseline-bench

Disclosed-baseline benchmark harness for **stock rust-libp2p gossipsub**
at large (4-10 MB) message sizes.

Nobody has published numbers for *tuned* stock gossipsub at 5 MB: the
upstream default `max_transmit_size` is 65536 (~80x under a 5 MB
payload), and published vendor comparisons (e.g. OptimumP2P's 2.05x
mean-latency claim) measure against an undisclosed baseline. This
harness makes every knob explicit and emits the full configuration next
to every result, so any number it produces is attributable.

## What it does

One process, N libp2p swarms (gossipsub `0.50.0-unreleased`, pinned rev
`ee8bf12e`) over the memory transport. Every connection is wrapped in a
fluid-model link shaper on the receive path: chunks become readable at
`max(arrival, serializer_free) + len/bandwidth + one_way_latency`.
Because shaping sits below yamux, flow-control windows back-pressure
through the emulated link. Node 0 publishes M seeded payloads of S
bytes; every node's first delivery is timestamped and wire bytes
(post-mux framing) are counted per node at the shaped layer.

Output: `results/summary.json` (config disclosure + p50/p90/p99/max
delivery latency, per-message completion, wire-byte amplification) and
`results/deliveries.jsonl` (every observation).

## Usage

```
cargo run --release -- --nodes=30 --message-bytes=5000000 --messages=10 \
  --latency-ms=50 --bandwidth-mbps=50 --mesh-n=8 --mesh-n-low=6 --mesh-n-high=12 \
  --idontwant-on-publish=false --flood-publish=false --validation=strict --seed=42
```

All flags are `--key=value`; defaults are the values above plus
`--edges-per-node=12 --heartbeat-ms=1000 --history-length=5
--history-gossip=3 --warmup-secs=10 --settle-secs=30 --out-dir=results`.

## Fidelity caveats (disclosed by design)

- **Real-time emulation, not virtual time**: latencies include host
  scheduling jitter; keep runs modest (<=100 nodes) and sequential.
- **Plaintext auth, not noise**: avoids in-process crypto CPU
  distorting latency at 5 MB x mesh-degree; disclosed in the config as
  `transport`.
- **Yamux default windows**: stream flow-control ceilings are part of
  stock behavior at 5 MB and are intentionally left at defaults
  (`yamux-default-windows` in the disclosure).
- **Signing on**: `signed` message authenticity, strict validation,
  matching production Ethereum-style deployments.
- The publish-call duration (signing + enqueue) is reported separately
  per message as `publish_call_ms`.

## License

MIT OR Apache-2.0.
