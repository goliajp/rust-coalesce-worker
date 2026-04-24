# coalesce-worker

[![Crates.io](https://img.shields.io/crates/v/coalesce-worker?style=flat-square&logo=rust)](https://crates.io/crates/coalesce-worker)
[![docs.rs](https://img.shields.io/docsrs/coalesce-worker?style=flat-square&logo=docs.rs)](https://docs.rs/coalesce-worker)
[![License](https://img.shields.io/crates/l/coalesce-worker?style=flat-square)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue?style=flat-square&logo=rust)](Cargo.toml)
[![Downloads](https://img.shields.io/crates/d/coalesce-worker?style=flat-square)](https://crates.io/crates/coalesce-worker)

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

世代カウンタ（generation counter）による古い結果の拒否を組み込んだ、合流（coalescing）型ワーカースレッド。tree-sitter や高価な計算をメインスレッド外で回すときに必須の作法で、すでに古くなった結果を現在の状態に適用してしまう事故を防ぎます。

**依存ゼロ**。もとは tree-sitter シンタックスハイライタから抽出しましたが、「このデータを処理する、新しいのが来たら捨てる」という任意のバックグラウンドタスクに適用可能です。

> **同義語 / 検索ワード**: リクエスト合流（request coalescing）、バックグラウンドワーカーのデバウンス、stale-result rejection、latest-wins タスクキュー、Rust tree-sitter 非同期ハイライト、古い解析ジョブのキャンセル。

## 問題

エディタがシンタックスハイライトをバックグラウンドスレッドに投げる。ユーザが速く打つと、メインスレッドはキー押下ごとにハイライト要求を送る。ワーカーは一度に 1 件しか処理できない。N-5 番目の要求が終わる頃にはソースは状態 N に進んでおり、古い N-5 から算出したスパンのバイトオフセットはもはや存在しない。メインスレッドがそれでも適用すると UI が崩れる。

これは `.claude/rules/common/classic-errors.md` に記録された **"stale async cache after mutation"** の失敗モードです。

## 修正

本 crate が強制する 2 つの作法：

- **リクエスト合流**: ワーカーは各ジョブの前にキューを空にし、まだ開始していない古い要求は黙って捨てます。
- **世代カウント**: 投入されたリクエストに単調増加の世代番号を付けます。`poll()` は保留中の結果をすべて取り出し、最新世代の 1 件だけを返し、古いものは破棄します。

## API

```rust
use coalesce_worker::{Coalescer, Worker, Output};

struct MyWorker;

impl Worker for MyWorker {
    type Request = String;
    type Response = usize;
    fn handle(&mut self, req: String) -> usize {
        // 高価な処理 —— tree-sitter パース、ハイライトなど
        req.len()
    }
}

let mut c = Coalescer::new(MyWorker);

// メインループ
let gen = c.submit("hello".to_string());

if let Some(Output { generation, value }) = c.poll() {
    // value を適用 —— これまで見た最新の投入に由来することが保証される
}
```

## コンテキスト切り替えの作法

バッファ切り替え（タブ切替、ファイルクローズ）時には、*前の* バッファ向けの処理中結果がまだ到着する可能性があります。適用してしまうと新しいバッファを壊します。切り替え時に `flush_pending()` を呼んでください：

```rust
c.flush_pending();
// その後、新しいコンテキストの要求を投入
```

## デモ

```bash
cargo run --example coalesce_demo -p coalesce-worker
```

1 件 150ms かかるワーカーに 20 件の要求を連射。各 submit と各 receive をプリントし、世代のギャップ（中間の要求が合流で消えて一度も実行されなかった証拠）を示します。

## インストール

```toml
[dependencies]
coalesce-worker = "0.1"
```

## 由来

[`goliajp/tora`](https://github.com/goliajp/tora) の `crates/tora-syntax/src/async_highlighter.rs` から抽出。GUI エディタの 19 言語 tree-sitter シンタックスハイライトを駆動していました。

<!-- ECOSYSTEM BEGIN (synced by claws/opensource/scripts/sync-ecosystem.py — edit ecosystem.toml, not this block) -->

## エコシステム

[metal-live-resize](https://crates.io/crates/metal-live-resize) · **coalesce-worker** · [damage-rects](https://crates.io/crates/damage-rects)

<!-- ECOSYSTEM END -->

## ライセンス

MIT —— [LICENSE](LICENSE) を参照。
