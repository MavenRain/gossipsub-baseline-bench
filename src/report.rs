//! Trace records, aggregate statistics, and hand-rolled JSON rendering
//! (fixed schema, no serde).

use crate::error::Error;
use std::time::Duration;

/// One first-delivery observation at a node.
#[derive(Clone, Debug)]
pub struct Delivery {
    node: usize,
    latency: Duration,
}

impl Delivery {
    /// Record a delivery at `node`, `latency` after the publish call.
    pub fn new(node: usize, latency: Duration) -> Self {
        Self { node, latency }
    }

    /// Receiving node index.
    pub fn node(&self) -> usize {
        self.node
    }

    /// Publish-call-to-delivery latency.
    pub fn latency(&self) -> Duration {
        self.latency
    }
}

/// Everything observed for one published message.
#[derive(Clone, Debug)]
pub struct MessageOutcome {
    seq: u64,
    publish_ok: bool,
    publish_err: String,
    publish_call: Duration,
    deliveries: Vec<Delivery>,
    expected: usize,
}

impl MessageOutcome {
    /// Assemble the outcome for message `seq`.
    pub fn new(
        seq: u64,
        publish_ok: bool,
        publish_err: String,
        publish_call: Duration,
        deliveries: Vec<Delivery>,
        expected: usize,
    ) -> Self {
        Self {
            seq,
            publish_ok,
            publish_err,
            publish_call,
            deliveries,
            expected,
        }
    }

    /// Message sequence number.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Whether every expected node delivered before the deadline.
    pub fn complete(&self) -> bool {
        self.publish_ok && self.deliveries.len() == self.expected
    }

    /// All recorded deliveries.
    pub fn deliveries(&self) -> impl Iterator<Item = &Delivery> {
        self.deliveries.iter()
    }

    /// Time the publisher reached every node (max delivery latency),
    /// when complete.
    pub fn completion(&self) -> Option<Duration> {
        self.complete()
            .then(|| self.deliveries.iter().map(Delivery::latency).max())
            .flatten()
    }

    fn json_line(&self) -> String {
        format!(
            "{{\"seq\":{},\"publish_ok\":{},\"publish_err\":\"{}\",\"publish_call_ms\":{:.3},\"delivered\":{},\"expected\":{},\"complete\":{}}}",
            self.seq,
            self.publish_ok,
            self.publish_err.replace('"', "'"),
            ms(self.publish_call),
            self.deliveries.len(),
            self.expected,
            self.complete(),
        )
    }
}

/// Full run result: disclosure, mesh state, per-message outcomes, bytes.
#[derive(Debug)]
pub struct Summary {
    disclosure: String,
    out_dir: String,
    mesh: Vec<(usize, usize, usize)>,
    outcomes: Vec<MessageOutcome>,
    node_bytes: Vec<(u64, u64)>,
    payload_bytes: usize,
    wall: Duration,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

/// Nearest-rank percentile over an ascending-sorted slice.
pub fn percentile(sorted: &[Duration], q: usize) -> Duration {
    let idx = sorted.len().saturating_sub(1).saturating_mul(q) / 100;
    sorted.get(idx).copied().unwrap_or_default()
}

impl Summary {
    /// Assemble the run summary.
    pub fn new(
        disclosure: String,
        out_dir: String,
        mesh: Vec<(usize, usize, usize)>,
        outcomes: Vec<MessageOutcome>,
        node_bytes: Vec<(u64, u64)>,
        payload_bytes: usize,
        wall: Duration,
    ) -> Self {
        Self {
            disclosure,
            out_dir,
            mesh,
            outcomes,
            node_bytes,
            payload_bytes,
            wall,
        }
    }

    fn sorted_latencies(&self) -> Vec<Duration> {
        let mut all: Vec<Duration> = self
            .outcomes
            .iter()
            .flat_map(|o| o.deliveries().map(Delivery::latency))
            .collect();
        all.sort();
        all
    }

    fn total_received(&self) -> u64 {
        self.node_bytes.iter().map(|(_, r)| r).sum()
    }

    fn ideal_bytes(&self) -> u64 {
        let receivers = u64::try_from(self.node_bytes.len().saturating_sub(1)).unwrap_or(0);
        let payload = u64::try_from(self.payload_bytes).unwrap_or(0);
        let msgs = u64::try_from(self.outcomes.len()).unwrap_or(0);
        receivers * payload * msgs
    }

    /// The JSON summary document.
    pub fn to_json(&self) -> String {
        let lat = self.sorted_latencies();
        let completions: Vec<Duration> = self
            .outcomes
            .iter()
            .filter_map(MessageOutcome::completion)
            .collect();
        let complete = self.outcomes.iter().filter(|o| o.complete()).count();
        let mesh_sizes: Vec<usize> = self.mesh.iter().map(|(_, m, _)| *m).collect();
        let mesh_min = mesh_sizes.iter().min().copied().unwrap_or(0);
        let mesh_max = mesh_sizes.iter().max().copied().unwrap_or(0);
        let dup = if self.ideal_bytes() > 0 {
            f64::from(u32::try_from(self.total_received() / self.ideal_bytes().max(1)).unwrap_or(u32::MAX))
                + (bytes_frac(self.total_received(), self.ideal_bytes()))
        } else {
            0.0
        };
        let outcome_lines: Vec<String> =
            self.outcomes.iter().map(MessageOutcome::json_line).collect();
        format!(
            concat!(
                "{{\"config\":{},\n",
                "\"mesh_degree_min\":{},\"mesh_degree_max\":{},\n",
                "\"messages_complete\":{},\"messages_total\":{},\n",
                "\"delivery_latency_ms\":{{\"p50\":{:.1},\"p90\":{:.1},\"p99\":{:.1},\"max\":{:.1}}},\n",
                "\"completion_ms\":{{\"p50\":{:.1},\"max\":{:.1}}},\n",
                "\"wire_bytes_received_total\":{},\"ideal_payload_bytes\":{},\"amplification\":{:.3},\n",
                "\"wall_seconds\":{:.1},\n",
                "\"messages\":[{}]}}"
            ),
            self.disclosure,
            mesh_min,
            mesh_max,
            complete,
            self.outcomes.len(),
            ms(percentile(&lat, 50)),
            ms(percentile(&lat, 90)),
            ms(percentile(&lat, 99)),
            ms(lat.last().copied().unwrap_or_default()),
            ms(percentile(&completions, 50)),
            ms(completions.iter().max().copied().unwrap_or_default()),
            self.total_received(),
            self.ideal_bytes(),
            dup,
            self.wall.as_secs_f64(),
            outcome_lines.join(",\n"),
        )
    }

    /// One JSONL line per delivery observation.
    pub fn deliveries_jsonl(&self) -> String {
        self.outcomes
            .iter()
            .flat_map(|o| {
                o.deliveries().map(move |d| {
                    format!(
                        "{{\"seq\":{},\"node\":{},\"latency_ms\":{:.3}}}",
                        o.seq(),
                        d.node(),
                        ms(d.latency())
                    )
                })
            })
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Output directory this run should persist into.
    pub fn out_dir(&self) -> &str {
        &self.out_dir
    }
}

fn bytes_frac(num: u64, den: u64) -> f64 {
    let scaled = num.saturating_mul(1000) / den.max(1) % 1000;
    f64::from(u32::try_from(scaled).unwrap_or(0)) / 1000.0
}

/// Write `summary.json` and `deliveries.jsonl` into the out dir.
pub fn persist(s: &Summary) -> Result<(), Error> {
    std::fs::create_dir_all(s.out_dir()).map_err(Error::Report)?;
    std::fs::write(format!("{}/summary.json", s.out_dir()), s.to_json()).map_err(Error::Report)?;
    std::fs::write(
        format!("{}/deliveries.jsonl", s.out_dir()),
        s.deliveries_jsonl(),
    )
    .map_err(Error::Report)
}

/// Human-readable digest printed to stdout.
pub fn render(s: &Summary) -> String {
    format!(
        "wrote {}/summary.json and deliveries.jsonl\n{}",
        s.out_dir(),
        s.to_json()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank() -> Result<(), String> {
        let v: Vec<Duration> = (1..=10).map(Duration::from_millis).collect();
        (percentile(&v, 50) == Duration::from_millis(5)
            && percentile(&v, 99) == Duration::from_millis(9)
            && percentile(&[], 50) == Duration::ZERO)
            .then_some(())
            .ok_or_else(|| "percentile math off".to_string())
    }
}
