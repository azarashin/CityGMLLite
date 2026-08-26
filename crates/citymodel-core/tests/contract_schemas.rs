use serde_json::Value;

use rusqlite::Connection;

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
    assert!(unity_constants.contains("DatabaseUserVersion = 1"));
}
