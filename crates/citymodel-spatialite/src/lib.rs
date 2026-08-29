//! Direct `SQLite` writer boundary for `CityGML` conversion output.

use rusqlite::{Connection, Transaction, params};
use std::path::Path;

pub const MODULE_NAME: &str = "citymodel-spatialite";
const INITIAL_SCHEMA: &str = include_str!("../../../contracts/sql/001_initial.sql");
const COMMON_FEATURES_SCHEMA: &str =
    include_str!("../../../contracts/sql/002_add_common_features.sql");
#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildingRow<'a> {
    pub building_id: &'a str,
    pub canonical_building_id: &'a str,
    pub gml_id: Option<&'a str>,
    pub source_file_id: i64,
    pub id_source: &'a str,
    pub id_is_synthetic: bool,
}

/// Creates one `SQLite` artifact directly from conversion events.
///
/// # Errors
///
/// Returns an error when schema initialization, writes, or integrity checks fail.
pub fn create_database(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.execute_batch(INITIAL_SCHEMA)?;
    connection.execute_batch(COMMON_FEATURES_SCHEMA)?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    Ok(connection)
}

/// Inserts a building within the caller's transaction and rejects duplicate IDs.
///
/// # Errors
///
/// Returns the database uniqueness error rather than overwriting a building.
pub fn insert_building(
    transaction: &Transaction<'_>,
    row: &BuildingRow<'_>,
) -> rusqlite::Result<()> {
    transaction.execute("INSERT INTO buildings (building_id, canonical_building_id, gml_id, id_source, id_is_synthetic, source_file_id, tile_id, local_feature_id, lod_used, lod_generated, centroid_x, centroid_y, footprint_quality) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'unassigned', 0, 1, 0, 0.0, 0.0, 'source')", params![row.building_id, row.canonical_building_id, row.gml_id, row.id_source, row.id_is_synthetic, row.source_file_id])?;
    Ok(())
}

/// Runs `SQLite`'s integrity check after conversion completes.
///
/// # Errors
///
/// Returns an error when `SQLite` reports corruption.
pub fn verify_integrity(connection: &Connection) -> rusqlite::Result<()> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result == "ok" {
        Ok(())
    } else {
        Err(rusqlite::Error::InvalidQuery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_indexed_database_and_rejects_duplicates() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(INITIAL_SCHEMA).unwrap();
        connection.execute_batch(COMMON_FEATURES_SCHEMA).unwrap();
        connection
            .pragma_update(None, "foreign_keys", "OFF")
            .unwrap();
        let row = BuildingRow {
            building_id: "b",
            canonical_building_id: "d::b",
            gml_id: Some("g"),
            source_file_id: 1,
            id_source: "gml",
            id_is_synthetic: false,
        };
        let transaction = connection.transaction().unwrap();
        insert_building(&transaction, &row).unwrap();
        assert!(insert_building(&transaction, &row).is_err());
        transaction.commit().unwrap();
        verify_integrity(&connection).unwrap();
    }

    #[test]
    fn rolls_back_buildings_when_a_transaction_fails() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(INITIAL_SCHEMA).unwrap();
        connection.execute_batch(COMMON_FEATURES_SCHEMA).unwrap();
        connection
            .pragma_update(None, "foreign_keys", "OFF")
            .unwrap();
        connection
            .execute_batch("INSERT INTO dataset_metadata (dataset_id, schema_version, generation_id, generated_at, generator_name, generator_version, source_crs_epsg, working_crs_epsg, vertical_reference_type, axis_order_json, dataset_origin_latitude, dataset_origin_longitude, dataset_origin_height, dataset_origin_geographic_epsg, dataset_origin_x, dataset_origin_y, dataset_origin_z, manifest_sha256, database_sha256, conversion_config_json, license_json) VALUES ('dataset', '1.0.0', 'generation', '1970-01-01T00:00:00Z', 'test', '0', 6697, 3857, 'source-defined', '[]', 0, 0, 0, 4326, 0, 0, 0, '0000000000000000000000000000000000000000000000000000000000000000', '0000000000000000000000000000000000000000000000000000000000000000', '{}', '{}');")
            .unwrap();
        let row = BuildingRow {
            building_id: "b",
            canonical_building_id: "dataset::b",
            gml_id: Some("g"),
            source_file_id: 1,
            id_source: "gml",
            id_is_synthetic: false,
        };
        {
            let transaction = connection.transaction().unwrap();
            insert_building(&transaction, &row).unwrap();
            assert!(insert_building(&transaction, &row).is_err());
        }
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM buildings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
