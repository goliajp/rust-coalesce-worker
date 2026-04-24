# coalesce-worker

[![Crates.io](https://img.shields.io/crates/v/coalesce-worker?style=flat-square&logo=rust)](https://crates.io/crates/coalesce-worker)
[![docs.rs](https://img.shields.io/docsrs/coalesce-worker?style=flat-square&logo=docs.rs)](https://docs.rs/coalesce-worker)
[![License](https://img.shields.io/crates/l/coalesce-worker?style=flat-square)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue?style=flat-square&logo=rust)](Cargo.toml)
[![Downloads](https://img.shields.io/crates/d/coalesce-worker?style=flat-square)](https://crates.io/crates/coalesce-worker)

[English](README.md) | **简体中文** | [日本語](README.ja.md)

带代际计数（generation counter）陈旧结果拒绝的合并式 worker 线程——把 tree-sitter（或任何昂贵计算）放到主线程外运行时必需的纪律，防止已过期的结果被应用到当前状态上。

**零依赖**。最初从 tree-sitter 语法高亮中抽出，但模式适用于任何"处理这份数据，若已有更新的数据则丢弃"的后台任务。

> **同义词 / 你可能在搜索**：请求合并（request coalescing）、后台 worker 去抖（debounce）、陈旧结果拒绝（stale-result rejection）、最新者胜（latest-wins）任务队列、Rust tree-sitter 异步高亮、取消被取代的解析任务。

## 问题

编辑器把语法高亮丢到后台线程。用户快速打字，主线程每次按键都发一个高亮请求。worker 一次只能处理一个。等第 N-5 个请求跑完时，源码已经到了第 N 个状态，根据陈旧的 N-5 源码算出来的 span 所指向的字节偏移已经不存在了。如果主线程还是把它们应用上去，UI 就会渲染错乱。

这就是 `.claude/rules/common/classic-errors.md` 里记录的 **"stale async cache after mutation"** 失败模式。

## 修复

本 crate 强制执行两条纪律：

- **请求合并**——worker 在每次工作前清空队列；还没开始执行的旧请求被静默丢弃。
- **代际计数**——每个提交的请求得到单调递增的代际编号。`poll()` 清空所有待返回结果只取最新的一个；旧的被丢弃。

## API

```rust
use coalesce_worker::{Coalescer, Worker, Output};

struct MyWorker;

impl Worker for MyWorker {
    type Request = String;
    type Response = usize;
    fn handle(&mut self, req: String) -> usize {
        // 昂贵计算 —— tree-sitter 解析、高亮等
        req.len()
    }
}

let mut c = Coalescer::new(MyWorker);

// 主循环
let gen = c.submit("hello".to_string());

if let Some(Output { generation, value }) = c.poll() {
    // 应用 value —— 保证来自目前见过的最新一次提交
}
```

## 上下文切换纪律

切换 buffer（切 tab、关文件）时，*之前* buffer 在飞的响应仍会到达。若被应用就会污染新 buffer。在上下文切换时调用 `flush_pending()`：

```rust
c.flush_pending();
// 然后提交新上下文的请求
```

## Demo

```bash
cargo run --example coalesce_demo -p coalesce-worker
```

向一个每次耗时 150ms 的 worker 连续提交 20 个请求。打印每次 submit + 每次 receive，展示代际间的空洞（证明中间请求被合并、从未执行）。

## 安装

```toml
[dependencies]
coalesce-worker = "0.1"
```

## 由来

从 [`goliajp/tora`](https://github.com/goliajp/tora) 的 `crates/tora-syntax/src/async_highlighter.rs` 抽出——在那里它驱动一个 GUI 编辑器对 19 种 tree-sitter 语言的语法高亮。

<!-- ECOSYSTEM BEGIN (synced by claws/opensource/scripts/sync-ecosystem.py — edit ecosystem.toml, not this block) -->

## 生态系统

[metal-live-resize](https://crates.io/crates/metal-live-resize) · **coalesce-worker** · [damage-rects](https://crates.io/crates/damage-rects)

<!-- ECOSYSTEM END -->

## 许可证

MIT —— 见 [LICENSE](LICENSE)。
