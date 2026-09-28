# Review Findings — AzureCanyon

## Review Pass: `src/config.rs`

- Traced settings loading and patching through `Config::patch_settings_with_roots`, CLI config commands, and tests.
- Found that settings patch writes used atomic rename but did not serialize read/merge/write across Pi processes.
- Fixed in `f3120138` by adding a process mutex, advisory lock file, parent-directory fsync, and a concurrent update regression test.

## Cross-Review: `75f0f6d9`

- Reviewed the compaction/permissions hardening commit for overflow, expiry-boundary, and persistence regressions.
- Found `PermissionStore` had the same read/merge/write lost-update race: two concurrently opened stores could overwrite each other's allow/deny decisions.
- Fixed in `d2d864af` by adding a process mutex, advisory lock file, reload-under-lock transaction, parent-directory fsync, and a concurrent record regression test.

## Validation

- `rch exec -- ... cargo fmt --check` passed.
- `rch exec -- ... cargo check --all-targets` passed.
- `rch exec -- ... cargo clippy --all-targets -- -D warnings` passed.
- Focused config regression passed: `cargo test --lib config::tests::patch_settings_serializes_concurrent_updates`.
- Full `cargo test` and broader `cargo test --lib` were stopped/failed under shared disk exhaustion; failures observed were `StorageFull` / `database or disk is full`, not assertion failures in the reviewed code.
