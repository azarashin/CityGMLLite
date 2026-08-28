use serde_json::Value;

use rusqlite::{Connection, params};

fn json(source: &str) -> Value {
    serde_json::from_str(source).expect("embedded JSON must parse")
}

fn assert_valid(schema_source: &str, instance_source: &str) {
    let schema = json(schema_source);
    let instance = json(instance_source);
    let validator = jsonschema::validator_for(&schema).expect("schema must compile");
    assert!(
        validator.validate(&instance).is_ok(),
        "fixture must validate: {:#?}",
        validator.iter_errors(&instance).collect::<Vec<_>>()
    );
}

#[test]
fn dataset_manifest_fixture_validates() {
    assert_valid(
        include_str!("../../../contracts/schemas/dataset-manifest.schema.json"),
        include_str!("fixtures/dataset.manifest.json"),
    );
}

#[test]
fn tile_metadata_fixture_validates() {
    assert_valid(
        include_str!("../../../contracts/schemas/tile-metadata.schema.json"),
        include_str!("fixtures/t_000012_000034.meta.json"),
    );
}

#[test]
fn manifest_rejects_dataset_escape() {
    let schema = json(include_str!(
        "../../../contracts/schemas/dataset-manifest.schema.json"
    ));
    let mut manifest = json(include_str!("fixtures/dataset.manifest.json"));
    manifest["database"]["path"] = Value::String("../citymodel.sqlite".to_owned());

    let validator = jsonschema::validator_for(&schema).expect("schema must compile");
    assert!(!validator.is_valid(&manifest));
}

#[test]
fn manifest_accepts_wkt_only_source_crs() {
    let schema = json(include_str!(
        "../../../contracts/schemas/dataset-manifest.schema.json"
    ));
    let mut manifest = json(include_str!("fixtures/dataset.manifest.json"));
    manifest["coordinateReference"]["sourceCrs"] = serde_json::json!({
        "wkt": "GEOGCRS[\"JGD2011\"]",
        "axisOrder": ["latitude", "longitude", "height"]
    });

    let validator = jsonschema::validator_for(&schema).expect("schema must compile");
    assert!(validator.is_valid(&manifest));
}

#[test]
fn initial_migration_creates_required_tables_and_guards() {
    let migration = include_str!("../../../contracts/sql/001_initial.sql");
    let connection = Connection::open_in_memory().expect("in-memory SQLite must open");
    connection
        .execute_batch(migration)
        .expect("initial migration must execute in SQLite");

    for table in [
        "dataset_metadata",
        "source_files",
        "tiles",
        "buildings",
        "building_parts",
        "building_attributes",
        "tile_features",
        "conversion_issues",
    ] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .expect("table existence query must succeed");
        assert!(exists, "migration must create {table}");
    }

    let application_id: i32 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .expect("application ID must be readable");
    let user_version: i32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user version must be readable");
    assert_eq!(application_id, 0x434D_4C54);
    assert_eq!(user_version, 1);
}

#[test]
fn manifest_accepts_type_specific_content_index_and_rejects_unsafe_metadata() {
    let schema = json(include_str!(
        "../../../contracts/schemas/dataset-manifest.schema.json"
    ));
    let mut manifest = json(include_str!("fixtures/dataset.manifest.json"));
    manifest["tiles"]["items"][0]["contents"] = serde_json::json!([{
        "featureType": "building",
        "metadata": "tiles/t_000012_000034.meta.json",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "byteLength": 123
    }]);
    let validator = jsonschema::validator_for(&schema).expect("schema must compile");
    assert!(validator.is_valid(&manifest));
    manifest["tiles"]["items"][0]["contents"][0]["metadata"] =
        Value::String("../tile.meta.json".to_owned());
    assert!(!validator.is_valid(&manifest));
}

#[test]
fn tile_metadata_accepts_common_feature_mapping() {
    let schema = json(include_str!(
        "../../../contracts/schemas/tile-metadata.schema.json"
    ));
    let mut metadata = json(include_str!("fixtures/t_000012_000034.meta.json"));
    metadata["features"]["items"] = serde_json::json!([{
        "localFeatureId": 0,
        "featureId": "01100-bldg-000001",
        "featureType": "building"
    }]);
    let validator = jsonschema::validator_for(&schema).expect("schema must compile");
    assert!(validator.is_valid(&metadata));
}

#[test]
fn common_feature_migration_creates_generic_tables_and_advances_version() {
    let initial = include_str!("../../../contracts/sql/001_initial.sql");
    let migration = include_str!("../../../contracts/sql/002_add_common_features.sql");
    let connection = Connection::open_in_memory().expect("in-memory SQLite must open");
    connection
        .execute_batch(initial)
        .expect("initial migration must execute");
    connection
        .execute_batch(migration)
        .expect("common feature migration must execute");
    for table in [
        "features",
        "feature_attributes",
        "tile_contents",
        "feature_tile_mappings",
    ] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .expect("table existence query must succeed");
        assert!(exists, "migration must create {table}");
    }
    let user_version: i32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(user_version, 2);
}

#[test]
fn common_feature_mapping_separates_local_ids_by_content_type() {
    let initial = include_str!("../../../contracts/sql/001_initial.sql");
    let migration = include_str!("../../../contracts/sql/002_add_common_features.sql");
    let connection = Connection::open_in_memory().expect("in-memory SQLite must open");
    connection.execute_batch(initial).unwrap();
    connection.execute_batch(migration).unwrap();
    connection.execute("INSERT INTO dataset_metadata (dataset_id, schema_version, generation_id, generated_at, generator_name, generator_version, source_crs_epsg, source_crs_wkt, working_crs_epsg, working_crs_wkt, vertical_crs_epsg, vertical_reference_type, axis_order_json, dataset_origin_latitude, dataset_origin_longitude, dataset_origin_height, dataset_origin_geographic_epsg, dataset_origin_x, dataset_origin_y, dataset_origin_z, manifest_sha256, database_sha256, conversion_config_json, license_json) VALUES ('d', '1.0.0', 'g', 'now', 'test', 'test', NULL, NULL, 3857, NULL, NULL, 'source-defined', '[]', 0, 0, 0, 4326, 0, 0, 0, ?1, ?1, '{}', '{}')", ["0".repeat(64)]).unwrap();
    connection.execute("INSERT INTO tiles (tile_id, dataset_id, generation_id, glb_relative_path, metadata_relative_path, glb_sha256, glb_byte_length, origin_latitude, origin_longitude, origin_height, origin_geographic_epsg, origin_x, origin_y, origin_z, tile_min_x, tile_min_y, tile_max_x, tile_max_y, content_min_x, content_min_y, content_min_z, content_max_x, content_max_y, content_max_z, projected_to_local_matrix_json, building_count, vertex_count, triangle_count, primitive_count) VALUES ('t', 'd', 'g', 'building.glb', 'building.meta.json', ?1, 0, 0, 0, 0, 4326, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, '[]', 0, 0, 0, 1)", ["0".repeat(64)]).unwrap();
    for feature_type in ["building", "terrain"] {
        connection.execute("INSERT INTO tile_contents (tile_id, feature_type, metadata_relative_path, metadata_sha256, metadata_byte_length, glb_relative_path, glb_sha256, glb_byte_length) VALUES ('t', ?1, 'content.meta.json', ?2, 0, 'content.glb', ?2, 0)", params![feature_type, "0".repeat(64)]).unwrap();
        connection.execute("INSERT INTO features (feature_id, canonical_feature_id, feature_type, gml_id, id_source, id_is_synthetic, source_file_id) VALUES (?1, ?2, ?1, NULL, 'gml', 0, NULL)", params![feature_type, format!("d::{feature_type}")]).unwrap();
        connection.execute("INSERT INTO feature_tile_mappings (tile_id, feature_type, local_feature_id, feature_id) VALUES ('t', ?1, 0, ?1)", [feature_type]).unwrap();
    }
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM feature_tile_mappings WHERE tile_id = 't' AND local_feature_id = 0", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 2);
}

#[test]
fn spatialite_extension_uses_selected_working_srid() {
    let extension = include_str!("../../../contracts/sql/001_initial.spatialite.sql");
    assert!(extension.contains("InitSpatialMetaData"));
    assert!(extension.contains("@working_srid@"));
    assert!(extension.contains("CreateSpatialIndex"));
}

#[test]
fn unity_contract_version_matches_contract_source() {
    let unity_constants = include_str!(
        "../../../Packages/com.azarashin.citymodel/Runtime/Versioning/CityModelContractVersion.cs"
    );
    assert!(unity_constants.contains("SchemaVersion = \"1.0.0\""));
    assert!(unity_constants.contains("GeneratorVersion = \"0.1.0-dev\""));
    assert!(unity_constants.contains("DatabaseUserVersion = 2"));
}
