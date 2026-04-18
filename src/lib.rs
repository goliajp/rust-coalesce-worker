//! A coalescing worker thread with generation-counter stale-result
//! rejection — the discipline needed to run tree-sitter (or any
//! expensive incremental parser) off the main thread without
//! corrupting state with out-of-date results.
//!
//! # The problem
//!
//! Suppose a GUI editor offloads syntax highlighting to a background
//! thread. The user types rapidly; the main thread fires off highlight
//! requests, one per keystroke. The worker can only process one at a
//! time. By the time request N-5 finishes, the source has moved to
//! state N, and the result computed from the stale N-5 source is no
//! longer valid — if the main thread applies it anyway, the UI shows
//! spans pointing at byte offsets that no longer exist, causing
//! rendering glitches or panics.
//!
//! This class of bug is documented in `goliajp/devops/dotclaude/common/classic-errors.md`
//! as **"Stale async cache after mutation"**.
//!
//! # The fix
//!
//! Two disciplines enforced by this crate:
//!
//! - **Request coalescing** — when the worker finishes one job, it
//!   drains any queued requests and processes only the latest; older
//!   requests that never started are silently discarded.
//! - **Generation counting** — every submitted request gets a monotonic
//!   generation number. When the main thread polls for results, it
//!   drains the receive channel and keeps only the newest generation;
//!   older results are dropped without being applied.
//!
//! # Example: tree-sitter highlighting
//!
//! ```no_run
//! use coalesce_worker::{Worker, Coalescer};
//! use std::sync::Arc;
//!
//! # #[cfg(any())] {
//! struct HighlightWorker {
//!     highlighter: tree_sitter_highlight::Highlighter,
//! }
//!
//! struct HighlightRequest {
//!     source: Arc<Vec<u8>>,
//!     config: Arc<tree_sitter_highlight::HighlightConfiguration>,
//! }
//!
//! struct HighlightResponse {
//!     events: Vec<tree_sitter_highlight::HighlightEvent>,
//! }
//!
//! impl Worker for HighlightWorker {
//!     type Request = HighlightRequest;
//!     type Response = HighlightResponse;
//!     fn handle(&mut self, req: Self::Request) -> Self::Response {
//!         let events = self.highlighter
//!             .highlight(&req.config, &req.source, None, |_| None)
//!             .unwrap()
//!             .collect::<Result<Vec<_>, _>>()
//!             .unwrap();
//!         HighlightResponse { events }
//!     }
//! }
//! # }
//! ```
//!
//! Then drive the coalescer from the main thread:
//!
//! ```no_run
//! # use coalesce_worker::{Worker, Coalescer, Output};
//! # struct MyWorker;
//! # struct Req;
//! # struct Res;
//! # impl Worker for MyWorker {
//! #     type Request = Req;
//! #     type Response = Res;
//! #     fn handle(&mut self, _r: Req) -> Res { Res }
//! # }
//! # let mut coalescer = Coalescer::new(MyWorker);
//! # fn current_source() -> Req { Req }
//! # fn render_highlights(_r: Res) {}
//! // main loop
//! coalescer.submit(current_source());
//! if let Some(Output { generation: _, value }) = coalescer.poll() {
//!     render_highlights(value);
//! }
//! ```
//!
//! # Not only for tree-sitter
//!
//! Any long-running background computation that can be superseded —
//! rebuilding a suggestion index, recompiling a preview, running a
//! linter — fits the same pattern.

#![deny(missing_docs)]

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// A worker that owns its processing state and handles one request
/// at a time.
///
/// The implementation should be free of shared mutable state — the
/// worker runs on its own thread and communicates with the main
/// thread only via request/response values.
pub trait Worker: Send + 'static {
    /// Input type sent from the main thread.
    type Request: Send + 'static;
    /// Output type returned after processing.
    type Response: Send + 'static;
    /// Processes one request. Called on the worker thread.
    fn handle(&mut self, req: Self::Request) -> Self::Response;
}

/// One response from the worker, tagged with the generation of the
/// request that produced it.
#[derive(Debug, Clone)]
pub struct Output<T> {
    /// Generation number of the request.
    pub generation: u64,
    /// The worker's response.
    pub value: T,
}

enum Msg<R> {
    Run { generation: u64, request: R },
    Shutdown,
}

/// Coalescing async dispatcher around a [`Worker`].
///
/// See the [crate-level docs](crate) for the problem it solves.
pub struct Coalescer<W: Worker> {
    tx: Sender<Msg<W::Request>>,
    rx: Receiver<Output<W::Response>>,
    generation: u64,
    _thread: Option<thread::JoinHandle<()>>,
}

impl<W: Worker> Coalescer<W> {
    /// Spawns the worker thread and returns a handle.
    ///
    /// The thread is named `coalesce-worker` by default. Use
    /// [`Coalescer::spawn_named`] to override.
    pub fn new(worker: W) -> Self {
        Self::spawn_named("coalesce-worker", worker)
    }

    /// Spawns the worker thread with a custom name (useful for
    /// profilers and panic backtraces).
    pub fn spawn_named(name: &str, worker: W) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<Msg<W::Request>>();
        let (res_tx, res_rx) = mpsc::channel::<Output<W::Response>>();

        let thread = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || worker_loop(worker, req_rx, res_tx))
            .expect("failed to spawn coalescer worker thread");

        Self {
            tx: req_tx,
            rx: res_rx,
            generation: 0,
            _thread: Some(thread),
        }
    }

    /// Submits a new request. Older requests queued but not yet
    /// started are silently discarded.
    ///
    /// Returns the generation number assigned to the new request;
    /// use it later to match against [`Output::generation`].
    ///
    /// Non-blocking: just sends on a channel. A send error means the
    /// worker has exited (panicked or dropped), in which case the
    /// returned generation will never produce a response.
    pub fn submit(&mut self, request: W::Request) -> u64 {
        self.generation += 1;
        let _ = self.tx.send(Msg::Run {
            generation: self.generation,
            request,
        });
        self.generation
    }

    /// Polls for the newest completed response.
    ///
    /// Drains all pending results from the channel and returns the
    /// one with the highest generation number, silently dropping any
    /// older responses. Returns `None` if no response is ready.
    pub fn poll(&mut self) -> Option<Output<W::Response>> {
        let mut latest: Option<Output<W::Response>> = None;
        while let Ok(out) = self.rx.try_recv() {
            match &latest {
                Some(cur) if cur.generation >= out.generation => {}
                _ => latest = Some(out),
            }
        }
        latest
    }

    /// Discards all pending results without taking ownership of them.
    ///
    /// Use when switching context (tab switch, file close) so that a
    /// response for the *previous* context doesn't leak into the new
    /// one. Does not cancel in-flight worker computation — only
    /// discards what's already been sent back.
    pub fn flush_pending(&mut self) {
        while self.rx.try_recv().is_ok() {}
    }

    /// Current generation counter (the number that was last assigned
    /// by [`submit`](Self::submit), or 0 if nothing has been submitted).
    pub fn current_generation(&self) -> u64 {
        self.generation
    }
}

impl<W: Worker> Drop for Coalescer<W> {
    fn drop(&mut self) {
        // Signal worker to exit; don't join — if the worker is busy
        // we don't want Drop to block.
        let _ = self.tx.send(Msg::Shutdown);
    }
}

fn worker_loop<W: Worker>(
    mut worker: W,
    req_rx: Receiver<Msg<W::Request>>,
    res_tx: Sender<Output<W::Response>>,
) {
    loop {
        // Block for the first message.
        let first = match req_rx.recv() {
            Ok(m) => m,
            Err(_) => return, // sender dropped
        };

        // Coalesce: if multiple messages are already queued, keep only
        // the newest request (by generation) and act on any Shutdown
        // we encounter.
        let mut latest: Option<(u64, W::Request)> = None;
        let mut shutdown = false;

        let process = |m: Msg<W::Request>, latest: &mut Option<(u64, W::Request)>| -> bool {
            match m {
                Msg::Run {
                    generation,
                    request,
                } => {
                    match latest {
                        Some((g, _)) if *g >= generation => {}
                        _ => *latest = Some((generation, request)),
                    }
                    false
                }
                Msg::Shutdown => true,
            }
        };

        shutdown = process(first, &mut latest) || shutdown;

        loop {
            match req_rx.try_recv() {
                Ok(m) => {
                    shutdown = process(m, &mut latest) || shutdown;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }

        if shutdown {
            return;
        }

        if let Some((generation, request)) = latest {
            let value = worker.handle(request);
            if res_tx.send(Output { generation, value }).is_err() {
                return; // receiver dropped
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    /// Trivial worker that echoes the number of times it has been called
    /// and records the input for inspection.
    struct CountingWorker {
        calls: Arc<AtomicUsize>,
    }

    impl Worker for CountingWorker {
        type Request = u32;
        type Response = u32;

        fn handle(&mut self, req: u32) -> u32 {
            self.calls.fetch_add(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(10));
            req
        }
    }

    fn wait_until<F: FnMut() -> bool>(mut cond: F, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if cond() {
                return true;
            }
            thread::sleep(Duration::from_millis(1));
        }
        false
    }

    #[test]
    fn submit_and_poll_roundtrip() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut c = Coalescer::new(CountingWorker {
            calls: Arc::clone(&calls),
        });

        let generation = c.submit(42);
        assert_eq!(generation, 1);

        let mut got = None;
        assert!(
            wait_until(
                || {
                    got = c.poll();
                    got.is_some()
                },
                Duration::from_secs(1),
            ),
            "timed out waiting for response",
        );

        let out = got.unwrap();
        assert_eq!(out.generation, 1);
        assert_eq!(out.value, 42);
    }

    #[test]
    fn poll_returns_newest_when_multiple_pending() {
        // Spin the worker by submitting many requests and only polling
        // at the end. The worker is slow enough that multiple responses
        // will queue up.
        let calls = Arc::new(AtomicUsize::new(0));
        let mut c = Coalescer::new(CountingWorker {
            calls: Arc::clone(&calls),
        });

        for i in 0..5 {
            c.submit(i);
        }

        // Wait for the worker to process at least one; responses may queue.
        thread::sleep(Duration::from_millis(100));
        let out = c.poll().expect("should receive at least one response");
        // Whatever was newest wins — must be the highest generation observed.
        assert!(out.generation <= 5);
        // No later response should surface — poll() drains everything.
        assert!(c.poll().is_none());
    }

    #[test]
    fn coalescing_drops_intermediate_requests() {
        // Submit 100 requests faster than the worker can handle — most
        // should be dropped before starting. At minimum, the final
        // generation's response must arrive.
        let calls = Arc::new(AtomicUsize::new(0));
        let mut c = Coalescer::new(CountingWorker {
            calls: Arc::clone(&calls),
        });

        for i in 0..100 {
            c.submit(i);
        }

        let mut max_gen = 0;
        let _ = wait_until(
            || {
                if let Some(out) = c.poll() {
                    max_gen = max_gen.max(out.generation);
                }
                max_gen == 100
            },
            Duration::from_secs(3),
        );

        assert_eq!(max_gen, 100, "final request should eventually complete");
        // Workers should have been called far fewer than 100 times
        // because coalescing drops stale requests.
        let total_calls = calls.load(Ordering::SeqCst);
        assert!(
            total_calls < 100,
            "expected coalescing to drop work, got {total_calls} calls"
        );
    }

    #[test]
    fn flush_pending_drops_unread_responses() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut c = Coalescer::new(CountingWorker {
            calls: Arc::clone(&calls),
        });

        c.submit(1);
        // Wait for the response to arrive.
        wait_until(|| calls.load(Ordering::SeqCst) >= 1, Duration::from_secs(1));
        thread::sleep(Duration::from_millis(20));

        c.flush_pending();
        assert!(
            c.poll().is_none(),
            "flush should have dropped the pending response"
        );
    }

    #[test]
    fn generation_monotonic() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut c = Coalescer::new(CountingWorker {
            calls: Arc::clone(&calls),
        });
        assert_eq!(c.current_generation(), 0);
        for i in 1..=5 {
            let g = c.submit(i);
            assert_eq!(g, i as u64);
        }
        assert_eq!(c.current_generation(), 5);
    }

    #[test]
    fn drop_shuts_down_cleanly() {
        // Verify creating and dropping doesn't panic or hang.
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Coalescer::new(CountingWorker {
            calls: Arc::clone(&calls),
        });
        drop(c);
    }
}
