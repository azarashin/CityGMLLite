# Output data format

`dataset.manifest.json` is the dataset entry point. Each tile has a neighboring
`.meta.json` file and a GLB. Both JSON documents must validate against the schemas
under `contracts/schemas/`; their `generationId` values must agree with GLB extras.

Feature IDs are tile-local `UNSIGNED_SHORT` values stored as `_FEATURE_ID_0` and map
by index to the metadata `features.buildingIds` array. The SQLite schema and migration
are the authoritative database definition in `contracts/sql/001_initial.sql`.
