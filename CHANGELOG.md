# Changelog

All notable changes to this crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-04-24

### Changed
- Smoke-test release via the new repo's GitHub Actions publish workflow.
  No code changes.

## [0.1.1] - 2026-04-24

### Changed
- Migrated from `goliajp/guirs` mono-repo to standalone `goliajp/rust-coalesce-worker`.
  No code changes; `repository` URL updated.

## [0.1.0] - 2026-04-18

### Added
- Initial release extracted from [goliajp/tora](https://github.com/goliajp/tora).
- `Worker` trait for background computation owned by its own thread.
- `Coalescer<W: Worker>` with `submit`, `poll`, `flush_pending`, and
  `current_generation`.
- Generation-counter stale-result rejection and request coalescing.
- `coalesce_demo` example showing generation gaps from coalesced work.
