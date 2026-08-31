//! In-process link emulation and neutral wire-byte accounting.
//!
//! `ShapedIo` wraps a raw connection and applies a fluid link model on
//! the receive path: each chunk read from the underlying channel is
//! released to the reader no earlier than
//! `max(arrival, serializer_free) + len/bandwidth + latency`.
//! Because shaping sits below yamux, stream flow-control windows
//! back-pressure through the emulated link exactly as they would over a
//! real one. Byte counters at this layer count wire bytes (after
//! multiplexing and framing), the neutral measure for duplication.

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::io::{AsyncRead, AsyncWrite};
use tokio::time::{sleep_until, Instant, Sleep};

/// Read granularity for the shaper (also the release-queue chunk size).
const CHUNK: usize = 16 * 1024;
/// Max chunks pulled from the inner channel per poll.
const MAX_DRAIN_ROUNDS: usize = 64;
/// Cap on bytes buffered in the emulated link before back-pressuring.
const MAX_QUEUE_BYTES: usize = 8 * 1024 * 1024;

fn bytes_f64(n: usize) -> f64 {
    u32::try_from(n).map(f64::from).unwrap_or(f64::MAX)
}

fn bytes_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Link bandwidth in bytes per second.
#[derive(Clone, Copy, Debug)]
pub struct BytesPerSec(f64);

impl BytesPerSec {
    /// Convert from megabits per second.
    pub fn from_mbps(mbps: f64) -> Self {
        Self(mbps * 1_000_000.0 / 8.0)
    }

    /// Serialization time for `n` bytes; zero when the rate is non-positive
    /// (treated as an infinitely fast link).
    pub fn service_time(&self, n: usize) -> Duration {
        if self.0 > 0.0 {
            Duration::from_secs_f64(bytes_f64(n) / self.0)
        } else {
            Duration::ZERO
        }
    }
}

/// Wire-byte counters for one node, summed over all its connections.
#[derive(Debug, Default)]
pub struct Counters {
    sent: AtomicU64,
    received: AtomicU64,
}

impl Counters {
    fn add_sent(&self, n: u64) {
        let _ = self.sent.fetch_add(n, Ordering::Relaxed);
    }

    fn add_received(&self, n: u64) {
        let _ = self.received.fetch_add(n, Ordering::Relaxed);
    }

    /// Current (sent, received) totals in bytes.
    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.sent.load(Ordering::Relaxed),
            self.received.load(Ordering::Relaxed),
        )
    }
}

/// Release schedule for one chunk under the fluid link model.
///
/// Returns `(release_at, serializer_free_after)`: the chunk finishes
/// serializing at `max(now, free_at) + service` and arrives `latency`
/// later.
pub fn schedule(
    now: Instant,
    free_at: Instant,
    service: Duration,
    latency: Duration,
) -> (Instant, Instant) {
    let start = now.max(free_at);
    let done = start + service;
    (done + latency, done)
}

struct Chunk {
    release_at: Instant,
    data: Vec<u8>,
    offset: usize,
}

enum ReadOutcome {
    Pending,
    Eof,
    Failed(io::Error),
    Got(usize),
}

/// A connection wrapped in the fluid link model plus byte counters.
pub struct ShapedIo<C> {
    inner: C,
    latency: Duration,
    rate: BytesPerSec,
    free_at: Instant,
    queue: VecDeque<Chunk>,
    queued_bytes: usize,
    timer: Option<Pin<Box<Sleep>>>,
    eof: bool,
    pending_err: Option<io::Error>,
    counters: Arc<Counters>,
}

impl<C> ShapedIo<C> {
    /// Wrap `inner` with the given one-way latency, bandwidth, and counters.
    pub fn new(inner: C, latency: Duration, rate: BytesPerSec, counters: Arc<Counters>) -> Self {
        Self {
            inner,
            latency,
            rate,
            free_at: Instant::now(),
            queue: VecDeque::new(),
            queued_bytes: 0,
            timer: None,
            eof: false,
            pending_err: None,
            counters,
        }
    }
}

impl<C: AsyncRead + Unpin> ShapedIo<C> {
    fn push_chunk(&mut self, data: Option<&[u8]>) {
        let _ = data.map(|d| {
            let now = Instant::now();
            let service = self.rate.service_time(d.len());
            let (release_at, free_after) = schedule(now, self.free_at, service, self.latency);
            self.free_at = free_after;
            self.queued_bytes += d.len();
            self.counters.add_received(bytes_u64(d.len()));
            self.queue.push_back(Chunk {
                release_at,
                data: d.to_vec(),
                offset: 0,
            });
        });
    }

    fn drain_once(&mut self, cx: &mut Context<'_>) -> ControlFlow<()> {
        let capped =
            self.eof || self.pending_err.is_some() || self.queued_bytes >= MAX_QUEUE_BYTES;
        if capped {
            ControlFlow::Break(())
        } else {
            let mut tmp = [0u8; CHUNK];
            let outcome = match Pin::new(&mut self.inner).poll_read(cx, &mut tmp) {
                Poll::Pending => ReadOutcome::Pending,
                Poll::Ready(res) => res.map_or_else(ReadOutcome::Failed, |n| {
                    if n == 0 {
                        ReadOutcome::Eof
                    } else {
                        ReadOutcome::Got(n)
                    }
                }),
            };
            match outcome {
                ReadOutcome::Pending => ControlFlow::Break(()),
                ReadOutcome::Eof => {
                    self.eof = true;
                    ControlFlow::Break(())
                }
                ReadOutcome::Failed(e) => {
                    self.pending_err = Some(e);
                    ControlFlow::Break(())
                }
                ReadOutcome::Got(n) => {
                    self.push_chunk(tmp.get(..n));
                    ControlFlow::Continue(())
                }
            }
        }
    }

    fn drain_inner(&mut self, cx: &mut Context<'_>) {
        let _ = (0..MAX_DRAIN_ROUNDS).try_for_each(|_| self.drain_once(cx));
    }

    fn copy_front(&mut self, buf: &mut [u8], written: usize) -> usize {
        let copied = self.queue.front_mut().map_or(0, |chunk| {
            let n = (buf.len() - written).min(chunk.data.len() - chunk.offset);
            let _ = buf
                .get_mut(written..written + n)
                .zip(chunk.data.get(chunk.offset..chunk.offset + n))
                .map(|(dst, src)| dst.copy_from_slice(src));
            chunk.offset += n;
            n
        });
        let exhausted = self
            .queue
            .front()
            .map(|c| c.offset >= c.data.len())
            .unwrap_or(false);
        if exhausted {
            let _ = self.queue.pop_front();
        }
        self.queued_bytes = self.queued_bytes.saturating_sub(copied);
        written + copied
    }

    fn deliver(&mut self, now: Instant, buf: &mut [u8]) -> usize {
        let flow = (0..buf.len().max(1)).try_fold(0usize, |written, _| {
            let ready = self
                .queue
                .front()
                .map(|c| c.release_at <= now)
                .unwrap_or(false);
            if ready && written < buf.len() {
                ControlFlow::Continue(self.copy_front(buf, written))
            } else {
                ControlFlow::Break(written)
            }
        });
        match flow {
            ControlFlow::Continue(w) => w,
            ControlFlow::Break(w) => w,
        }
    }

    fn take_err(&mut self) -> io::Error {
        self.pending_err
            .take()
            .unwrap_or_else(|| io::Error::other("shaped-io: error already taken"))
    }

    fn poll_armed_timer(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let fired = self
            .timer
            .as_mut()
            .map(|t| t.as_mut().poll(cx).is_ready())
            .unwrap_or(false);
        if fired {
            let n = self.deliver(Instant::now(), buf);
            if n > 0 {
                Poll::Ready(Ok(n))
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        } else {
            Poll::Pending
        }
    }

    fn poll_blocked(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let front_at = self.queue.front().map(|c| c.release_at);
        let _ = front_at.map(|at| self.timer = Some(Box::pin(sleep_until(at))));
        match () {
            () if front_at.is_some() => self.poll_armed_timer(cx, buf),
            () if self.pending_err.is_some() => Poll::Ready(Err(self.take_err())),
            () if self.eof => Poll::Ready(Ok(0)),
            () => Poll::Pending,
        }
    }
}

impl<C: AsyncRead + Unpin> AsyncRead for ShapedIo<C>
where
    Self: Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.drain_inner(cx);
        let n = this.deliver(Instant::now(), buf);
        if n > 0 {
            Poll::Ready(Ok(n))
        } else {
            this.poll_blocked(cx, buf)
        }
    }
}

impl<C: AsyncWrite + Unpin> AsyncWrite for ShapedIo<C>
where
    Self: Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let counters = this.counters.clone();
        Pin::new(&mut this.inner).poll_write(cx, buf).map_ok(|n| {
            counters.add_sent(bytes_u64(n));
            n
        })
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_close(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_serializes_back_to_back() -> Result<(), String> {
        let now = Instant::now();
        let latency = Duration::from_millis(50);
        let service = Duration::from_millis(10);
        let (r1, f1) = schedule(now, now, service, latency);
        (r1 == now + service + latency && f1 == now + service)
            .then_some(())
            .ok_or_else(|| "first chunk mistimed".to_string())?;
        let (r2, f2) = schedule(now, f1, service, latency);
        (r2 == now + service + service + latency && f2 == now + service + service)
            .then_some(())
            .ok_or_else(|| "second chunk did not queue behind the first".to_string())
    }

    #[test]
    fn schedule_idles_when_link_free() -> Result<(), String> {
        let now = Instant::now();
        let earlier = now - Duration::from_secs(1);
        let latency = Duration::from_millis(5);
        let service = Duration::from_millis(2);
        let (r, f) = schedule(now, earlier, service, latency);
        (r == now + service + latency && f == now + service)
            .then_some(())
            .ok_or_else(|| "idle link should serialize from now".to_string())
    }

    #[test]
    fn service_time_scales_with_rate() -> Result<(), String> {
        let rate = BytesPerSec::from_mbps(8.0);
        let t = rate.service_time(1_000_000);
        (t == Duration::from_secs(1))
            .then_some(())
            .ok_or_else(|| format!("8 Mbps over 1 MB should be 1s, got {t:?}"))
    }
}
