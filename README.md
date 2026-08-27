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

## Convert and render a PLATEAU CityGML file

The converter accepts either one `.gml` file or a PLATEAU dataset root with
`udx/bldg/`. It writes a self-contained directory containing the manifest, 500 m
GLB tiles, tile metadata, and SQLite.

```powershell
cargo run -p citymodel-cli -- convert `
  "C:\CityGMLLiteData\input\53396479_bldg_6697_op.gml" `
  --output "C:\CityGMLLiteData\output\saitama-lod1" `
  --tolerant
```

Open `UnityProject/Assets/QuickStart.unity` in Unity 6.5.9f1, set **Dataset
Root** on **CityModel Quick Start** to the generated directory, and enter Play
mode. The sample verifies tile hashes and creates a Unity Mesh for every tile.

The initial E2E path derives a Web Mercator (EPSG:3857) working plane from
PLATEAU EPSG:6697/6668 geographic coordinates. It is intended for runtime
visualization, not survey-grade JGD2011 plane-rectangular accuracy.

## Current status

The MVP modules now provide CityGML streaming, geometry and tile processing, GLB
writing, SQLite output, and Unity runtime boundaries. Advanced CRS accuracy,
full CityGML surface semantics, textures, and production-scale streaming remain
follow-up work.

## Data contracts

Issue #1 defines JSON Schemas, SQLite/SpatiaLite migration SQL, and the contract
version policy under `contracts/`. The schemas are validated against fixture
datasets by the Rust test suite.
