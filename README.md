# coalesce-worker

[![Crates.io](https://img.shields.io/crates/v/coalesce-worker?style=flat-square&logo=rust)](https://crates.io/crates/coalesce-worker)
[![docs.rs](https://img.shields.io/docsrs/coalesce-worker?style=flat-square&logo=docs.rs)](https://docs.rs/coalesce-worker)
[![License](https://img.shields.io/crates/l/coalesce-worker?style=flat-square)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue?style=flat-square&logo=rust)](Cargo.toml)
[![Downloads](https://img.shields.io/crates/d/coalesce-worker?style=flat-square)](https://crates.io/crates/coalesce-worker)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

A coalescing worker thread with generation-counter stale-result
rejection — the discipline needed to run tree-sitter (or any expensive
computation) off the main thread without applying out-of-date results.

**Zero dependencies.** Originally extracted from a tree-sitter syntax
highlighter, but the pattern fits any "process this data, discard if
newer arrived" background task.

> **Also known as / if you're searching for:** request coalescing,
> debouncing a background worker, stale-result rejection, latest-wins
> task queue, async tree-sitter highlighting in Rust, canceling
> superseded parse jobs.

## The problem

An editor offloads syntax highlighting to a background thread. The user
types fast; the main thread fires off highlight requests, one per
keystroke. The worker can only process one at a time. By the time
request N-5 finishes, the source is at state N and the spans computed
from the stale N-5 source point at byte offsets that no longer exist.
If the main thread applies them anyway the UI corrupts.

This is the **"stale async cache after mutation"** failure mode
documented in `.claude/rules/common/classic-errors.md`.

## The fix

Two disciplines enforced by this crate:

- **Request coalescing** — the worker drains its queue before each job;
  older requests that never started are silently dropped.
- **Generation counting** — every submitted request gets a monotonic
  generation number. `poll()` drains all pending results and returns
  only the newest; older results are discarded.

## API

```rust
use coalesce_worker::{Coalescer, Worker, Output};

struct MyWorker;

impl Worker for MyWorker {
    type Request = String;
    type Response = usize;
    fn handle(&mut self, req: String) -> usize {
        // expensive work here — tree-sitter parse, highlight, etc.
        req.len()
    }
}

let mut c = Coalescer::new(MyWorker);

// main loop
let gen = c.submit("hello".to_string());

if let Some(Output { generation, value }) = c.poll() {
    // apply `value` — guaranteed to be from the newest submission seen
}
```

## Using it with tree-sitter

```rust,ignore
use std::sync::Arc;
use coalesce_worker::{Coalescer, Worker};
use tree_sitter_highlight::{HighlightConfiguration, Highlighter, HighlightEvent};

struct HighlightWorker {
    highlighter: Highlighter,
}

struct HighlightRequest {
    source: Arc<Vec<u8>>,
    config: Arc<HighlightConfiguration>,
}

struct HighlightResponse {
    events: Vec<HighlightEvent>,
}

impl Worker for HighlightWorker {
    type Request = HighlightRequest;
    type Response = HighlightResponse;
    fn handle(&mut self, req: Self::Request) -> Self::Response {
        let events = self
            .highlighter
            .highlight(&req.config, &req.source, None, |_| None)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        HighlightResponse { events }
    }
}

let mut c = Coalescer::new(HighlightWorker { highlighter: Highlighter::new() });
// ... per keystroke: c.submit(HighlightRequest { source, config });
// ... per frame: c.poll();
```

## Context-switch discipline

When switching buffers (tab change, file close), any in-flight response
for the *previous* buffer will still arrive. If applied, it corrupts
the new buffer. Call `flush_pending()` on context switch:

```rust
# use coalesce_worker::{Coalescer, Worker};
# struct W; impl Worker for W { type Request = (); type Response = (); fn handle(&mut self, _: ()) {} }
# let mut c = Coalescer::new(W);
c.flush_pending();
// then submit requests for the new context
```

## Demo

```bash
cargo run --example coalesce_demo -p coalesce-worker
```

Submits 20 requests rapid-fire to a worker that takes 150ms per job.
Prints each submit + each receive, showing generation gaps (proof
that intermediate requests were coalesced away and never ran).

## Install

```toml
[dependencies]
coalesce-worker = "0.1"
```

## Origin

Extracted from [`goliajp/tora`](https://github.com/goliajp/tora) —
`crates/tora-syntax/src/async_highlighter.rs`, where it drove
syntax highlighting for 19 tree-sitter languages in a GUI editor.

<!-- ECOSYSTEM BEGIN (generated — edit ecosystem.toml, not this block) -->

## Ecosystem

[metal-live-resize](https://crates.io/crates/metal-live-resize) · **coalesce-worker** · [damage-rects](https://crates.io/crates/damage-rects)

<!-- ECOSYSTEM END -->

## License

MIT — see [LICENSE](LICENSE).
