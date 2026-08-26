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

## Initial migration

1. Execute `sql/001_initial.sql` inside the converter's creation transaction.
2. Load only the pinned SpatiaLite bridge and execute
   `sql/001_initial.spatialite.sql` after replacing `@working_srid@`.
3. Run `PRAGMA integrity_check` and checkpoint WAL before publishing the dataset.

The database contract is read-only at runtime. Unity must check both
`schemaVersion` and `generationId` against the manifest before opening it.
