//! Visual demo of request coalescing.
//!
//! Submits 20 requests rapid-fire to a worker that's slow enough that
//! they can't all run sequentially. Coalescing ensures only a handful
//! actually execute — the rest are dropped before they start because
//! a newer request has already arrived.
//!
//! Run: `cargo run --example coalesce_demo -p coalesce-worker`.

use coalesce_worker::{Coalescer, Worker};
use std::thread;
use std::time::{Duration, Instant};

/// A worker that simulates an expensive computation (e.g. tree-sitter
/// parsing + highlighting on a large file).
struct SlowWorker {
    calls: u32,
}

impl Worker for SlowWorker {
    type Request = u32;
    type Response = RunStat;

    fn handle(&mut self, req: u32) -> RunStat {
        self.calls += 1;
        let start = Instant::now();
        thread::sleep(Duration::from_millis(150));
        RunStat {
            request: req,
            total_calls: self.calls,
            duration: start.elapsed(),
        }
    }
}

#[derive(Debug)]
struct RunStat {
    request: u32,
    total_calls: u32,
    duration: Duration,
}

fn main() {
    println!("coalesce-worker: coalescing demo");
    println!("---------------------------------");
    println!("worker takes 150ms per request; we submit every 20ms.\n");

    let mut c = Coalescer::new(SlowWorker { calls: 0 });

    // Submit 20 requests faster than the worker can process them.
    for i in 0..20 {
        let generation = c.submit(i);
        println!("  submit  gen={generation:<3}  value={i}");
        thread::sleep(Duration::from_millis(20));
    }

    println!("\n  (all submitted; letting worker drain)\n");

    // Poll for responses. A generation gap between receipts proves
    // older generations were coalesced away and never ran.
    let mut received = Vec::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Some(out) = c.poll() {
            println!(
                "  receive gen={:<3}  req={:<3}  worker_call_count={:<3}  dur={:?}",
                out.generation, out.value.request, out.value.total_calls, out.value.duration
            );
            received.push(out.generation);
            if out.generation >= 20 {
                break;
            }
        }
        thread::sleep(Duration::from_millis(30));
    }

    println!(
        "\nResult: received {} responses for 20 submitted requests.",
        received.len()
    );
    println!(
        "Dropped generations (never ran): {} of 20.",
        20 - received.len()
    );
    if let (Some(first), Some(last)) = (received.first(), received.last()) {
        println!("Generation gaps prove coalescing worked: smallest = {first}, largest = {last}.");
    }
}
