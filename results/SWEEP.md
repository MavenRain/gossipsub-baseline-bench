# Baseline sweep, 2026-08-30

Stock rust-libp2p gossipsub (rev ee8bf12e), 30 nodes, edges-per-node 12, mesh 6/8/12, strict validation, signed messages, seed 42, warmup 12 s. Transport: memory + plaintext + yamux default windows. All 12 runs completed with full delivery (no wedged runs, no incomplete messages).

| run | size | msgs | lat ms | bw Mbps | idw | complete | p50 ms | p90 ms | p99 ms | max ms | comp p50 | comp max | amp | wall s |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| a-4mb-idw0 | 4 MB | 10 | 50 | 50 | off | 10/10 | 1477.7 | 2309.3 | 3510.0 | 3863.7 | 1582.8 | 3863.7 | 5.822 | 34.8 |
| a-4mb-idw1 | 4 MB | 10 | 50 | 50 | on | 10/10 | 1512.1 | 2351.8 | 3021.0 | 3512.3 | 1723.8 | 3512.3 | 6.039 | 39.6 |
| a-5mb-idw0 | 5 MB | 10 | 50 | 50 | off | 10/10 | 1954.1 | 2642.3 | 2919.8 | 3320.2 | 3088.5 | 3320.2 | 5.984 | 41.8 |
| a-5mb-idw1 | 5 MB | 10 | 50 | 50 | on | 10/10 | 1816.3 | 2657.5 | 2939.7 | 3113.6 | 1983.5 | 3113.6 | 5.839 | 38.3 |
| a-8mb-idw0 | 8 MB | 5 | 50 | 50 | off | 5/5 | 2847.5 | 3932.6 | 4835.3 | 6288.8 | 3258.3 | 6288.8 | 5.777 | 38.5 |
| a-8mb-idw1 | 8 MB | 5 | 50 | 50 | on | 5/5 | 2816.6 | 3433.8 | 3932.9 | 3976.0 | 3867.1 | 3976.0 | 5.759 | 32.9 |
| a-10mb-idw0 | 10 MB | 5 | 50 | 50 | off | 5/5 | 3483.5 | 4351.0 | 5153.5 | 5304.3 | 4302.9 | 5304.3 | 5.821 | 37.7 |
| a-10mb-idw1 | 10 MB | 5 | 50 | 50 | on | 5/5 | 3594.0 | 4742.6 | 5263.0 | 5521.7 | 4351.8 | 5521.7 | 5.485 | 40.7 |
| b-5mb-l25-b25 | 5 MB | 5 | 25 | 25 | off | 5/5 | 3250.6 | 3384.9 | 3441.3 | 3453.2 | 3410.9 | 3453.2 | 6.387 | 31.5 |
| b-5mb-l25-b100 | 5 MB | 5 | 25 | 100 | off | 5/5 | 976.6 | 1264.2 | 1733.4 | 1818.0 | 1818.0 | 1818.0 | 5.584 | 22.0 |
| b-5mb-l100-b25 | 5 MB | 5 | 100 | 25 | off | 5/5 | 3450.3 | 4379.3 | 4492.1 | 4519.1 | 3573.1 | 4519.1 | 6.273 | 34.7 |
| b-5mb-l100-b100 | 5 MB | 5 | 100 | 100 | off | 5/5 | 1156.2 | 2558.2 | 2851.0 | 3539.5 | 2270.6 | 3539.5 | 6.370 | 24.8 |

`amp` = wire_bytes_received_total / ideal_payload_bytes. `comp` = per-message completion (all 29 receivers).

## IDONTWANT-on-publish deltas (idw on minus idw off)

| size | p50 ms | p99 ms | max ms | amplification |
|---|---|---|---|---|
| 4 MB | +34.4 | -489.0 | -351.4 | +0.217 |
| 5 MB | -137.8 | +19.9 | -206.6 | -0.145 |
| 8 MB | -30.9 | -902.4 | -2312.8 | -0.018 |
| 10 MB | +110.5 | +109.5 | +217.4 | -0.336 |

Pattern: the median effect is inside run-to-run noise (both signs, under 140 ms). The consistent wins are in the tail (p99/max shrink at 4, 5, 8 MB; the 8 MB max drops 2.3 s) and in wire volume at the biggest size (10 MB amplification 5.821 -> 5.485, about 5.8% less traffic). At 4 MB idw on cost slightly more wire (+0.217): the payload clears the mesh before IDONTWANT can cancel much.

## Latency/bandwidth grid at 5 MB (idw off)

| | bw 25 | bw 100 |
|---|---|---|
| lat 25 | p50 3250.6, amp 6.387 | p50 976.6, amp 5.584 |
| lat 100 | p50 3450.3, amp 6.273 | p50 1156.2, amp 6.370 |

Bandwidth dominates: 25 -> 100 Mbps cuts p50 by ~2.3 s at either latency. Link latency 25 -> 100 ms adds only ~180 ms to p50 but widens the tail at high bandwidth (p90 1264 -> 2558). Amplification runs higher whenever delivery is slow (6.27-6.39) than in the fast corner (5.58): the longer a payload is in flight, the more duplicate mesh transmissions start before caches converge.

## Notes

- The 8 MiB ShapedIo queue cap sat below the 10 MB payload; backpressure blocked as designed and both 10 MB runs completed. No stall observed.
- Amplification is stable at 5.5-6.4 across all cells, roughly mesh-degree-shaped duplicate delivery for every payload size; the eager-push duplicate cost is the headline inefficiency of stock gossipsub at these sizes.
