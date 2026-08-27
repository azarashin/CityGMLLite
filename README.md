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
cargo run -p citymodel-cli -- inspect path/to/input.gml
```

The Unity package can be added from disk by selecting
`Packages/com.azarashin.citymodel/package.json` in Unity Package Manager.

## Current status

The MVP modules now provide CityGML streaming, geometry and tile processing, GLB
writing, SQLite output, and Unity runtime boundaries. See `docs/` for operation
guides and the currently known limitations.

## Data contracts

Issue #1 defines JSON Schemas, SQLite/SpatiaLite migration SQL, and the contract
version policy under `contracts/`. The schemas are validated against fixture
datasets by the Rust test suite.
