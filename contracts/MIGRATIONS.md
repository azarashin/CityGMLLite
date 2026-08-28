# Data-contract migration policy

`contracts/version.json` is the source of truth for the public data contract.

- `schemaVersion` is a semantic version stored in JSON metadata.
- `databaseUserVersion` is an integer stored in SQLite `PRAGMA user_version`.
- Every database migration is append-only and named with a zero-padded integer,
  for example `002_add_attribute_catalog.sql`.
- The converter records applied migrations in `schema_migrations` and rejects a
  database whose `user_version` is newer than it supports.
- Breaking JSON changes increment the schema major version and require a
  versioned migration or an explicit incompatibility error in Unity.

## Current migration chain

1. Execute `sql/001_initial.sql` inside the converter's creation transaction.
2. Execute `sql/002_add_common_features.sql`. It retains the building-specific
   tables for v1 consumers and adds `features`, `feature_attributes`,
   `tile_contents`, and `feature_tile_mappings` for all feature types. A local
   feature ID is scoped by `(tile_id, feature_type)`, so independent building
   and terrain content can both use local ID `0` in the same tile.
3. Load only the pinned SpatiaLite bridge and execute
   `sql/001_initial.spatialite.sql` after replacing `@working_srid@`.
4. Run `PRAGMA integrity_check` and checkpoint WAL before publishing the dataset.

The database contract is read-only at runtime. Unity must check both
`schemaVersion` and `generationId` against the manifest before opening it.
The current Unity building bridge continues to use the v1 building tables and
accepts database user versions 1 and 2. Generic feature queries are introduced
with user version 2 and require the common-feature runtime API.
