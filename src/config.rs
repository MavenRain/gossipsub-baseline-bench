//! Benchmark configuration. Every knob is explicit, every knob is
//! disclosed in the emitted report; nothing rides on library defaults
//! silently.

use crate::error::Error;
use std::time::Duration;

/// Message validation policy, mirroring gossipsub's modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validation {
    /// Full signature + fields validation (default).
    Strict,
    /// Signatures validated when present.
    Permissive,
    /// Signatures must be absent.
    Anonymous,
    /// No validation at all.
    Off,
}

impl Validation {
    fn parse(v: &str) -> Option<Self> {
        match v {
            "strict" => Some(Validation::Strict),
            "permissive" => Some(Validation::Permissive),
            "anonymous" => Some(Validation::Anonymous),
            "off" => Some(Validation::Off),
            _ => None,
        }
    }

    /// Stable label used in the disclosure JSON.
    pub fn label(&self) -> &'static str {
        match self {
            Validation::Strict => "strict",
            Validation::Permissive => "permissive",
            Validation::Anonymous => "anonymous",
            Validation::Off => "off",
        }
    }
}

/// Full harness configuration. Constructed via `parse` or `Default`.
#[derive(Clone, Debug)]
pub struct BenchConfig {
    nodes: usize,
    edges_per_node: usize,
    mesh_n: usize,
    mesh_n_low: usize,
    mesh_n_high: usize,
    message_bytes: usize,
    messages: u64,
    latency_ms: u64,
    bandwidth_mbps: f64,
    flood_publish: bool,
    idontwant_on_publish: bool,
    validation: Validation,
    heartbeat_ms: u64,
    history_length: usize,
    history_gossip: usize,
    warmup_secs: u64,
    settle_secs: u64,
    seed: u64,
    out_dir: String,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            nodes: 30,
            edges_per_node: 12,
            mesh_n: 8,
            mesh_n_low: 6,
            mesh_n_high: 12,
            message_bytes: 5_000_000,
            messages: 10,
            latency_ms: 50,
            bandwidth_mbps: 50.0,
            flood_publish: false,
            idontwant_on_publish: false,
            validation: Validation::Strict,
            heartbeat_ms: 1_000,
            history_length: 5,
            history_gossip: 3,
            warmup_secs: 10,
            settle_secs: 30,
            seed: 42,
            out_dir: "results".to_string(),
        }
    }
}

fn parsed<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, Error>
where
    T::Err: std::fmt::Display,
{
    v.parse().map_err(|e: T::Err| Error::InvalidFlag {
        flag: flag.to_string(),
        reason: e.to_string(),
    })
}

impl BenchConfig {
    /// Parse `--key=value` flags on top of the defaults.
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Self, Error> {
        args.fold(Ok(Self::default()), |acc, arg| {
            acc.and_then(|cfg| cfg.apply(&arg))
        })
    }

    fn apply(self, arg: &str) -> Result<Self, Error> {
        let (key, value) = arg.split_once('=').ok_or_else(|| Error::InvalidFlag {
            flag: arg.to_string(),
            reason: "expected --key=value".to_string(),
        })?;
        let mut cfg = self;
        match key {
            "--nodes" => cfg.nodes = parsed(key, value)?,
            "--edges-per-node" => cfg.edges_per_node = parsed(key, value)?,
            "--mesh-n" => cfg.mesh_n = parsed(key, value)?,
            "--mesh-n-low" => cfg.mesh_n_low = parsed(key, value)?,
            "--mesh-n-high" => cfg.mesh_n_high = parsed(key, value)?,
            "--message-bytes" => cfg.message_bytes = parsed(key, value)?,
            "--messages" => cfg.messages = parsed(key, value)?,
            "--latency-ms" => cfg.latency_ms = parsed(key, value)?,
            "--bandwidth-mbps" => cfg.bandwidth_mbps = parsed(key, value)?,
            "--flood-publish" => cfg.flood_publish = parsed(key, value)?,
            "--idontwant-on-publish" => cfg.idontwant_on_publish = parsed(key, value)?,
            "--heartbeat-ms" => cfg.heartbeat_ms = parsed(key, value)?,
            "--history-length" => cfg.history_length = parsed(key, value)?,
            "--history-gossip" => cfg.history_gossip = parsed(key, value)?,
            "--warmup-secs" => cfg.warmup_secs = parsed(key, value)?,
            "--settle-secs" => cfg.settle_secs = parsed(key, value)?,
            "--seed" => cfg.seed = parsed(key, value)?,
            "--out-dir" => cfg.out_dir = value.to_string(),
            "--validation" => {
                cfg.validation = Validation::parse(value).ok_or_else(|| Error::InvalidFlag {
                    flag: key.to_string(),
                    reason: "expected strict|permissive|anonymous|off".to_string(),
                })?
            }
            other => Err(Error::UnknownFlag(other.to_string()))?,
        }
        cfg.validate()
    }

    fn validate(self) -> Result<Self, Error> {
        let ok = self.nodes >= 2
            && self.message_bytes >= 8
            && self.messages >= 1
            && self.mesh_n_low <= self.mesh_n
            && self.mesh_n <= self.mesh_n_high
            && self.bandwidth_mbps > 0.0;
        ok.then_some(self).ok_or_else(|| Error::InvalidFlag {
            flag: "(combined)".to_string(),
            reason: "need nodes>=2, message-bytes>=8, messages>=1, \
                     mesh-n-low<=mesh-n<=mesh-n-high, bandwidth>0"
                .to_string(),
        })
    }

    /// Node count.
    pub fn nodes(&self) -> usize {
        self.nodes
    }
    /// Connection budget per node (>= mesh_n_high to leave room for the mesh).
    pub fn edges_per_node(&self) -> usize {
        self.edges_per_node
    }
    /// Gossipsub D.
    pub fn mesh_n(&self) -> usize {
        self.mesh_n
    }
    /// Gossipsub D_lo.
    pub fn mesh_n_low(&self) -> usize {
        self.mesh_n_low
    }
    /// Gossipsub D_hi.
    pub fn mesh_n_high(&self) -> usize {
        self.mesh_n_high
    }
    /// Benchmark payload size in bytes.
    pub fn message_bytes(&self) -> usize {
        self.message_bytes
    }
    /// Number of measured messages.
    pub fn messages(&self) -> u64 {
        self.messages
    }
    /// One-way link propagation delay.
    pub fn latency(&self) -> Duration {
        Duration::from_millis(self.latency_ms)
    }
    /// Per-link bandwidth in megabits per second.
    pub fn bandwidth_mbps(&self) -> f64 {
        self.bandwidth_mbps
    }
    /// Gossipsub flood_publish.
    pub fn flood_publish(&self) -> bool {
        self.flood_publish
    }
    /// Gossipsub idontwant_on_publish (ships default false upstream).
    pub fn idontwant_on_publish(&self) -> bool {
        self.idontwant_on_publish
    }
    /// Validation mode.
    pub fn validation(&self) -> Validation {
        self.validation
    }
    /// Gossipsub heartbeat interval.
    pub fn heartbeat(&self) -> Duration {
        Duration::from_millis(self.heartbeat_ms)
    }
    /// Gossipsub history_length.
    pub fn history_length(&self) -> usize {
        self.history_length
    }
    /// Gossipsub history_gossip.
    pub fn history_gossip(&self) -> usize {
        self.history_gossip
    }
    /// Mesh warm-up wait before measuring.
    pub fn warmup(&self) -> Duration {
        Duration::from_secs(self.warmup_secs)
    }
    /// Per-message completion deadline.
    pub fn settle(&self) -> Duration {
        Duration::from_secs(self.settle_secs)
    }
    /// RNG seed (topology, keypairs, payloads).
    pub fn seed(&self) -> u64 {
        self.seed
    }
    /// Report output directory.
    pub fn out_dir(&self) -> &str {
        &self.out_dir
    }
    /// Gossipsub max_transmit_size actually configured (2x payload;
    /// the upstream default of 65536 would reject these payloads).
    pub fn max_transmit_size(&self) -> usize {
        self.message_bytes.saturating_mul(2)
    }

    /// Full disclosure of every knob, as a JSON object string.
    pub fn disclosure_json(&self) -> String {
        format!(
            concat!(
                "{{\"nodes\":{},\"edges_per_node\":{},\"mesh_n\":{},",
                "\"mesh_n_low\":{},\"mesh_n_high\":{},\"message_bytes\":{},",
                "\"messages\":{},\"latency_ms\":{},\"bandwidth_mbps\":{},",
                "\"flood_publish\":{},\"idontwant_on_publish\":{},",
                "\"validation\":\"{}\",\"heartbeat_ms\":{},\"history_length\":{},",
                "\"history_gossip\":{},\"max_transmit_size\":{},\"warmup_secs\":{},",
                "\"settle_secs\":{},\"seed\":{},",
                "\"transport\":\"memory+plaintext+yamux-default-windows\",",
                "\"signing\":\"signed\",\"libp2p_rev\":\"ee8bf12e6d94d48518ea67773abb11625b2c4f41\"}}"
            ),
            self.nodes,
            self.edges_per_node,
            self.mesh_n,
            self.mesh_n_low,
            self.mesh_n_high,
            self.message_bytes,
            self.messages,
            self.latency_ms,
            self.bandwidth_mbps,
            self.flood_publish,
            self.idontwant_on_publish,
            self.validation.label(),
            self.heartbeat_ms,
            self.history_length,
            self.history_gossip,
            self.max_transmit_size(),
            self.warmup_secs,
            self.settle_secs,
            self.seed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_overrides_and_rejects() -> Result<(), String> {
        let cfg = BenchConfig::parse(
            ["--nodes=10", "--message-bytes=1000000", "--validation=permissive"]
                .into_iter()
                .map(str::to_string),
        )
        .map_err(|e| e.to_string())?;
        (cfg.nodes() == 10
            && cfg.message_bytes() == 1_000_000
            && cfg.validation() == Validation::Permissive)
            .then_some(())
            .ok_or_else(|| "override not applied".to_string())?;
        BenchConfig::parse(["--bogus=1".to_string()].into_iter())
            .err()
            .map(|_| ())
            .ok_or_else(|| "unknown flag accepted".to_string())
    }
}
