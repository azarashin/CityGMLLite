# MVP verification and performance plan

## Automated in CI

- Rust formatting, Clippy, unit tests, and the small CityGML golden fixture run in `.github/workflows/verify.yml`.
- The fixture checks prefix-independent namespaces, LOD1 coordinates, `srsName`, `srsDimension`, and deterministic event output.
- Contract fixtures validate `dataset.manifest.json` and tile metadata schemas.

## Reference-dataset run (manual until #20 / #24)

Record one run per PLATEAU dataset version in `docs/performance-results/` using this table.

| Dataset | Files / buildings | CLI elapsed / peak memory | GLB bytes / tiles | SQLite size / integrity | Warnings / errors | Notes |
| --- | ---: | --- | --- | --- | --- | --- |
| _not yet run_ | | | | | | |

## Unity 6 manual verification

- Open `dataset.manifest.json`; reject bad schemaVersion or generationId.
- Load a tile from filesystem and StreamingAssets; compare GLB SHA-256 to metadata.
- Confirm a `_FEATURE_ID_0` value maps to the metadata BuildingID.
- Move Scene Origin and confirm GLB local coordinates are unchanged.
- Unload the tile and inspect Mesh, Material, buffer, and collider release in the Unity Profiler.

## Current limitation

No licensed or downloaded PLATEAU reference dataset is committed to this repository. Real-data conversion, Unity integration, fuzzing, and performance target measurements remain release-gate work under #20 and facade work under #24.
