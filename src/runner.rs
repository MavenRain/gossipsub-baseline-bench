//! Swarm construction and the benchmark schedule: build N shaped nodes,
//! dial the topology, warm the mesh, publish M messages from node 0,
//! and collect first-delivery timestamps plus wire-byte deltas.

use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc;
use futures::future::ready;
use futures::stream::{self, StreamExt, TryStreamExt};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use tokio::time::Instant;

use libp2p_core::multiaddr::Protocol;
use libp2p_core::transport::MemoryTransport;
use libp2p_core::upgrade;
use libp2p_core::{Multiaddr, Transport};
use libp2p_gossipsub::{
    Behaviour, ConfigBuilder, Event, IdentTopic, MessageAuthenticity, ValidationMode,
};
use libp2p_swarm::{Config as SwarmConfig, Swarm, SwarmEvent};

use crate::config::{BenchConfig, Validation};
use crate::error::Error;
use crate::report::{Delivery, MessageOutcome, Summary};
use crate::shaping::{BytesPerSec, Counters, ShapedIo};
use crate::topology::Topology;

enum Command {
    Dial(Multiaddr),
    Publish { seq: u64, data: Vec<u8> },
    MeshReport,
    Shutdown,
}

enum Up {
    Listening,
    Published {
        seq: u64,
        ok: bool,
        err: String,
        call: Duration,
    },
    Delivered {
        node: usize,
        seq: u64,
        at: Instant,
    },
    Mesh {
        node: usize,
        mesh: usize,
        connected: usize,
    },
}

impl Up {
    fn listening(self) -> Option<()> {
        match self {
            Up::Listening => Some(()),
            Up::Published { .. } | Up::Delivered { .. } | Up::Mesh { .. } => None,
        }
    }

    fn mesh(self) -> Option<(usize, usize, usize)> {
        match self {
            Up::Mesh {
                node,
                mesh,
                connected,
            } => Some((node, mesh, connected)),
            Up::Published { .. } | Up::Delivered { .. } | Up::Listening => None,
        }
    }

    fn measurement(self, want_seq: u64) -> Option<Up> {
        match self {
            Up::Published { seq, .. } => (seq == want_seq).then_some(self),
            Up::Delivered { seq, .. } => (seq == want_seq).then_some(self),
            Up::Mesh { .. } | Up::Listening => None,
        }
    }
}

fn memory_addr(idx: usize) -> Multiaddr {
    Multiaddr::empty().with(Protocol::Memory(u64::try_from(idx).unwrap_or(0) + 1))
}

fn to_gossipsub(v: Validation) -> ValidationMode {
    match v {
        Validation::Strict => ValidationMode::Strict,
        Validation::Permissive => ValidationMode::Permissive,
        Validation::Anonymous => ValidationMode::Anonymous,
        Validation::Off => ValidationMode::None,
    }
}

fn deterministic_keypair(seed: u64, idx: usize) -> Result<libp2p_identity::Keypair, Error> {
    let stream = u64::try_from(idx).unwrap_or(0);
    let mut rng = ChaCha8Rng::seed_from_u64(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ stream);
    let bytes: [u8; 32] = rng.gen();
    libp2p_identity::Keypair::ed25519_from_bytes(bytes)
        .map_err(|e| Error::Config(format!("keypair: {e}")))
}

fn payload(seed: u64, seq: u64, size: usize) -> Vec<u8> {
    let rng = ChaCha8Rng::seed_from_u64(seed ^ seq.wrapping_mul(0x2545_f491_4f6c_dd1d));
    seq.to_le_bytes()
        .into_iter()
        .chain(
            rng.sample_iter(rand::distributions::Standard)
                .take(size.saturating_sub(8)),
        )
        .collect()
}

fn build_node(
    idx: usize,
    cfg: &BenchConfig,
    counters: Arc<Counters>,
    topic: &IdentTopic,
) -> Result<Swarm<Behaviour>, Error> {
    let keypair = deterministic_keypair(cfg.seed(), idx)?;
    let peer_id = keypair.public().to_peer_id();
    let latency = cfg.latency();
    let rate = BytesPerSec::from_mbps(cfg.bandwidth_mbps());
    let transport = MemoryTransport::default()
        .map(move |chan, _| ShapedIo::new(chan, latency, rate, counters.clone()))
        .upgrade(upgrade::Version::V1)
        .authenticate(libp2p_plaintext::Config::new(&keypair))
        .multiplex(libp2p_yamux::Config::default())
        .boxed();
    let gcfg = ConfigBuilder::default()
        .max_transmit_size(cfg.max_transmit_size())
        .validation_mode(to_gossipsub(cfg.validation()))
        .mesh_n(cfg.mesh_n())
        .mesh_n_low(cfg.mesh_n_low())
        .mesh_n_high(cfg.mesh_n_high())
        .flood_publish(cfg.flood_publish())
        .idontwant_on_publish(cfg.idontwant_on_publish())
        .heartbeat_interval(cfg.heartbeat())
        .history_length(cfg.history_length())
        .history_gossip(cfg.history_gossip())
        .duplicate_cache_time(Duration::from_secs(120))
        .build()
        .map_err(|e| Error::Config(format!("{e:?}")))?;
    let behaviour =
        Behaviour::new(MessageAuthenticity::Signed(keypair), gcfg).map_err(Error::Behaviour)?;
    let mut swarm = Swarm::new(
        transport,
        behaviour,
        peer_id,
        SwarmConfig::with_tokio_executor(),
    );
    let _ = swarm
        .behaviour_mut()
        .subscribe(topic)
        .map_err(|e| Error::Subscribe(format!("{e:?}")))?;
    let _ = swarm
        .listen_on(memory_addr(idx))
        .map_err(|e| Error::Listen(format!("{e:?}")))?;
    Ok(swarm)
}

fn handle_cmd(
    idx: usize,
    swarm: &mut Swarm<Behaviour>,
    cmd: Option<Command>,
    tx: &mpsc::UnboundedSender<Up>,
    topic: &IdentTopic,
) -> bool {
    let _ = idx;
    cmd.map_or(true, |c| match c {
        Command::Dial(addr) => {
            let _ = swarm.dial(addr);
            false
        }
        Command::Publish { seq, data } => {
            let t0 = Instant::now();
            let res = swarm.behaviour_mut().publish(topic.clone(), data);
            let call = t0.elapsed();
            let _ = tx.unbounded_send(Up::Published {
                seq,
                ok: res.is_ok(),
                err: res.err().map(|e| format!("{e:?}")).unwrap_or_default(),
                call,
            });
            false
        }
        Command::MeshReport => {
            let mesh = swarm.behaviour().mesh_peers(&topic.hash()).count();
            let connected = swarm.behaviour().all_peers().count();
            let _ = tx.unbounded_send(Up::Mesh {
                node: idx,
                mesh,
                connected,
            });
            false
        }
        Command::Shutdown => true,
    })
}

fn handle_event(idx: usize, ev: SwarmEvent<Event>, tx: &mpsc::UnboundedSender<Up>) {
    if let SwarmEvent::Behaviour(Event::Message { message, .. }) = &ev {
        let _ = message
            .data
            .first_chunk::<8>()
            .map(|b| u64::from_le_bytes(*b))
            .map(|seq| {
                tx.unbounded_send(Up::Delivered {
                    node: idx,
                    seq,
                    at: Instant::now(),
                })
            });
    }
    if let SwarmEvent::NewListenAddr { .. } = &ev {
        let _ = tx.unbounded_send(Up::Listening);
    }
}

async fn pump(
    idx: usize,
    swarm: Swarm<Behaviour>,
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    ev_tx: mpsc::UnboundedSender<Up>,
    topic: IdentTopic,
) {
    stream::unfold(
        (swarm, cmd_rx, ev_tx, topic, false),
        move |(mut swarm, mut rx, tx, topic, done)| async move {
            if done {
                None
            } else {
                let next_done = {
                    futures::select! {
                        cmd = rx.next() => handle_cmd(idx, &mut swarm, cmd, &tx, &topic),
                        ev = swarm.select_next_some() => {
                            handle_event(idx, ev, &tx);
                            false
                        }
                    }
                };
                Some(((), (swarm, rx, tx, topic, next_done)))
            }
        },
    )
    .for_each(|()| ready(()))
    .await;
}

async fn await_count<T>(
    rx: &mut mpsc::UnboundedReceiver<Up>,
    want: usize,
    deadline: Duration,
    phase: &'static str,
    mut f: impl FnMut(Up) -> Option<T>,
) -> Result<Vec<T>, Error> {
    let got: Vec<T> = rx
        .by_ref()
        .take_until(Box::pin(tokio::time::sleep(deadline)))
        .filter_map(move |ev| ready(f(ev)))
        .take(want)
        .collect()
        .await;
    (got.len() == want)
        .then_some(got)
        .ok_or(Error::SetupTimeout(phase))
}

async fn run_message(
    rx: &mut mpsc::UnboundedReceiver<Up>,
    publisher: &mpsc::UnboundedSender<Command>,
    cfg: &BenchConfig,
    seq: u64,
) -> Result<MessageOutcome, Error> {
    let data = payload(cfg.seed(), seq, cfg.message_bytes());
    let expected = cfg.nodes().saturating_sub(1);
    let sent_at = Instant::now();
    publisher
        .unbounded_send(Command::Publish { seq, data })
        .map_err(|_| Error::ChannelClosed("publisher command channel"))?;
    let events: Vec<Up> = rx
        .by_ref()
        .take_until(Box::pin(tokio::time::sleep(cfg.settle())))
        .filter_map(|ev| ready(ev.measurement(seq)))
        .take(expected + 1)
        .collect()
        .await;
    let (ok, err, call) = events
        .iter()
        .filter_map(|e| match e {
            Up::Published { ok, err, call, .. } => Some((*ok, err.clone(), *call)),
            Up::Delivered { .. } | Up::Mesh { .. } | Up::Listening => None,
        })
        .next()
        .unwrap_or((false, "publish event missing".to_string(), Duration::ZERO));
    let deliveries: Vec<Delivery> = events
        .iter()
        .filter_map(|e| match e {
            Up::Delivered { node, at, .. } => Some(Delivery::new(*node, *at - sent_at)),
            Up::Published { .. } | Up::Mesh { .. } | Up::Listening => None,
        })
        .collect();
    Ok(MessageOutcome::new(seq, ok, err, call, deliveries, expected))
}

async fn orchestrate(cfg: BenchConfig) -> Result<Summary, Error> {
    let started = Instant::now();
    let n = cfg.nodes();
    let topic = IdentTopic::new("baseline-bench");
    let counters: Vec<Arc<Counters>> = (0..n).map(|_| Arc::new(Counters::default())).collect();
    let (ev_tx, mut ev_rx) = mpsc::unbounded();
    let cmd_txs: Vec<mpsc::UnboundedSender<Command>> = counters
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let swarm = build_node(i, &cfg, c.clone(), &topic)?;
            let (tx, rx) = mpsc::unbounded();
            let _ = tokio::spawn(pump(i, swarm, rx, ev_tx.clone(), topic.clone()));
            Ok(tx)
        })
        .collect::<Result<Vec<_>, Error>>()?;
    drop(ev_tx);

    let _ = await_count(&mut ev_rx, n, Duration::from_secs(10), "listening", Up::listening)
        .await?;

    let topo = Topology::ring_plus_random(n, cfg.edges_per_node(), cfg.seed());
    topo.edges().for_each(|(a, b)| {
        let _ = cmd_txs
            .get(a)
            .map(|tx| tx.unbounded_send(Command::Dial(memory_addr(b))));
    });

    tokio::time::sleep(cfg.warmup()).await;

    cmd_txs.iter().for_each(|tx| {
        let _ = tx.unbounded_send(Command::MeshReport);
    });
    let mesh =
        await_count(&mut ev_rx, n, Duration::from_secs(10), "mesh report", Up::mesh).await?;

    let base: Vec<(u64, u64)> = counters.iter().map(|c| c.snapshot()).collect();

    let publisher = cmd_txs
        .first()
        .ok_or(Error::ChannelClosed("no publisher"))?
        .clone();
    let (outcomes, _rx) = stream::iter(0..cfg.messages())
        .map(Ok::<u64, Error>)
        .try_fold(
            (Vec::new(), ev_rx),
            |(acc, mut rx), seq| {
                let publisher = publisher.clone();
                let cfg = cfg.clone();
                async move {
                    let outcome = run_message(&mut rx, &publisher, &cfg, seq).await?;
                    let grown: Vec<MessageOutcome> =
                        acc.into_iter().chain(std::iter::once(outcome)).collect();
                    Ok((grown, rx))
                }
            },
        )
        .await?;

    tokio::time::sleep(Duration::from_secs(2)).await;
    let node_bytes: Vec<(u64, u64)> = counters
        .iter()
        .zip(base.iter())
        .map(|(c, (bs, br))| {
            let (s, r) = c.snapshot();
            (s.saturating_sub(*bs), r.saturating_sub(*br))
        })
        .collect();
    cmd_txs.iter().for_each(|tx| {
        let _ = tx.unbounded_send(Command::Shutdown);
    });

    let mut mesh_sorted = mesh;
    mesh_sorted.sort();
    Ok(Summary::new(
        cfg.disclosure_json(),
        cfg.out_dir().to_string(),
        mesh_sorted,
        outcomes,
        node_bytes,
        cfg.message_bytes(),
        started.elapsed(),
    ))
}

/// Build the tokio runtime and run the full benchmark schedule.
pub fn run(cfg: BenchConfig) -> Result<Summary, Error> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    rt.block_on(orchestrate(cfg))
}
