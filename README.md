# CityGMLLite

CityGMLLite converts PLATEAU CityGML into lightweight GLB tiles, dataset metadata,
and SQLite + SpatiaLite for use by a Unity runtime package.

## Repository layout

- `crates/`: Rust converter libraries and the `citymodel` CLI.
- `contracts/`: versioned cross-runtime data-contract sources.
- `Packages/com.azarashin.citymodel/`: Unity 6 UPM package.
- `Documents/`: requirements and design documents.

## Development checks

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p citymodel-cli
```

The Unity package can be added from disk by selecting
`Packages/com.azarashin.citymodel/package.json` in Unity Package Manager.

## Current status

This is the repository skeleton for GitHub issue #4. It defines module boundaries
only; CityGML parsing, geometry conversion, GLB writing, database integration, and
Unity runtime behavior are implemented by their dedicated MVP issues.
