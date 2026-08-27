# Build and known limitations

Run the Rust checks from the repository root:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The Unity package requires Unity 6 and is added from its local `package.json`.

## Known MVP limitations

- No licensed full PLATEAU reference dataset or converted sample scene is committed.
- Unity package behavior has not been executed in a Unity editor in this repository.
- The Windows x64 SQLite bridge is an API contract; a packaged native bridge remains
  release validation work.
- Phase 2/3 enhancements are tracked in Issues #22 and #21.
