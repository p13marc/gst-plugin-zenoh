# Release Plan: gst-plugin-zenoh v0.3.0

This document outlines improvements identified for the 0.3.0 release.

## Critical (Must Fix)

### 1. Add Minimum Supported Rust Version (MSRV)
- [ ] Add `rust-version = "1.82"` to Cargo.toml (required for edition 2024)
- [ ] Document MSRV in README.md

## High Priority

### 2. Fix Broken CHANGELOG Links
The CHANGELOG references documentation files that don't exist:
- `COMPRESSION.md`
- `METADATA_FEATURES.md`
- `SMART_CAPS_TRANSMISSION.md`
- `DYNAMIC_RECONFIGURATION_ANALYSIS.md`

Options:
- [ ] **Option A**: Remove references from CHANGELOG (simpler)
- [ ] **Option B**: Create these docs in a `docs/` folder

### 3. Audit `.unwrap()` and `.expect()` in Core Paths
Files to audit:
- [ ] `src/zenohsink/imp.rs` - replace panics with proper error handling
- [ ] `src/zenohsrc/imp.rs` - replace panics with proper error handling
- [ ] `src/zenohdemux/imp.rs` - replace panics with proper error handling

Note: `.unwrap()` in builders (mod.rs) is acceptable since they're infallible.

### 4. Check Unused Dependencies
- [ ] Verify if `anyhow` is used anywhere in the codebase
- [ ] Remove if unused

## Medium Priority

### 5. API Consistency Between Elements
Consider adding to ZenohSrc for symmetry with ZenohSink:
- [ ] Document auto-decompression behavior explicitly
- [ ] Consider `caps-timeout-ms` property for waiting on caps from sender

### 6. Add Missing Tests
- [ ] Network timeout recovery test
- [ ] Malformed buffer metadata handling test
- [ ] Compression error propagation test
- [ ] Concurrent element creation stress test

### 7. Lock Dependency Versions
In Cargo.toml:
- [ ] Change `zenoh = "1"` to `zenoh = "1.0"` (lock minor version)
- [ ] Review other dependency version constraints

### 8. Document Feature Combinations
Add to README or element READMEs:
- [ ] What happens if receiver doesn't have compression feature but receives compressed data?
- [ ] Can you mix compressed and uncompressed streams?

## Low Priority

### 9. Code Organization
- [ ] Add section comments to large imp.rs files for better navigation

### 10. Test Organization
- [ ] Extract common `init()` function to `tests/common/mod.rs`

### 11. Documentation Polish
- [ ] Document element rank selection rationale in code comments
- [ ] Standardize rustdoc example formatting (use `no_run` consistently)

### 12. CHANGELOG Preparation
- [ ] Add v0.3.0 section with planned changes

## Not Doing (Deferred)

### Session Sharing
The analysis mentioned session sharing as a potential feature. This is complex and should be deferred to a future release:
- Requires significant API changes
- Current per-element sessions work well
- Listed in ARCHITECTURE.md as "Future Optimization"

## Release Checklist

Before publishing 0.3.0:
- [ ] All critical and high priority items complete
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo doc` generates without warnings
- [ ] Update version in Cargo.toml to `0.3.0`
- [ ] Update CHANGELOG.md with release date
- [ ] Tag release in git
- [ ] Publish to crates.io

## Estimated Effort

| Priority | Items | Effort |
|----------|-------|--------|
| Critical | 1 | 10 min |
| High | 3 | 1-2 hours |
| Medium | 4 | 2-3 hours |
| Low | 4 | 1 hour |
| **Total** | **12** | **~5 hours** |
