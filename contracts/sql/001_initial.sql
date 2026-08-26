-- CityGMLLite initial SQLite schema.
--
-- Execute this migration in a transaction before the companion SpatiaLite migration.
-- @working_srid@ is substituted by the converter with the selected Working CRS EPSG code.

PRAGMA foreign_keys = ON;
PRAGMA application_id = 0x434D4C54; -- "CMLT"
PRAGMA user_version = 1;

CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL,
    description TEXT NOT NULL
);

INSERT INTO schema_migrations (version, applied_at, description)
VALUES (1, CURRENT_TIMESTAMP, 'Initial CityGMLLite schema');

CREATE TABLE dataset_metadata (
    dataset_id TEXT PRIMARY KEY,
    schema_version TEXT NOT NULL,
    generation_id TEXT NOT NULL UNIQUE,
    generated_at TEXT NOT NULL,
    generator_name TEXT NOT NULL,
    generator_version TEXT NOT NULL,
    source_crs_epsg INTEGER,
    source_crs_wkt TEXT,
    working_crs_epsg INTEGER NOT NULL,
    working_crs_wkt TEXT,
    vertical_crs_epsg INTEGER,
    vertical_reference_type TEXT NOT NULL,
    axis_order_json TEXT NOT NULL,
    dataset_origin_latitude REAL NOT NULL,
    dataset_origin_longitude REAL NOT NULL,
    dataset_origin_height REAL NOT NULL,
    dataset_origin_geographic_epsg INTEGER NOT NULL,
    dataset_origin_x REAL NOT NULL,
    dataset_origin_y REAL NOT NULL,
    dataset_origin_z REAL NOT NULL,
    manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64),
    database_sha256 TEXT NOT NULL CHECK (length(database_sha256) = 64),
    conversion_config_json TEXT NOT NULL,
    license_json TEXT NOT NULL
);

CREATE TABLE source_files (
    source_file_id INTEGER PRIMARY KEY,
    dataset_id TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    FOREIGN KEY (dataset_id) REFERENCES dataset_metadata(dataset_id),
    UNIQUE (dataset_id, relative_path)
);

CREATE TABLE tiles (
    tile_id TEXT PRIMARY KEY,
    dataset_id TEXT NOT NULL,
    generation_id TEXT NOT NULL,
    glb_relative_path TEXT NOT NULL,
    metadata_relative_path TEXT NOT NULL,
    glb_sha256 TEXT NOT NULL CHECK (length(glb_sha256) = 64),
    glb_byte_length INTEGER NOT NULL CHECK (glb_byte_length >= 0),
    origin_latitude REAL NOT NULL,
    origin_longitude REAL NOT NULL,
    origin_height REAL NOT NULL,
    origin_geographic_epsg INTEGER NOT NULL,
    origin_x REAL NOT NULL,
    origin_y REAL NOT NULL,
    origin_z REAL NOT NULL,
    tile_min_x REAL NOT NULL,
    tile_min_y REAL NOT NULL,
    tile_max_x REAL NOT NULL,
    tile_max_y REAL NOT NULL,
    content_min_x REAL NOT NULL,
    content_min_y REAL NOT NULL,
    content_min_z REAL NOT NULL,
    content_max_x REAL NOT NULL,
    content_max_y REAL NOT NULL,
    content_max_z REAL NOT NULL,
    projected_to_local_matrix_json TEXT NOT NULL,
    building_count INTEGER NOT NULL CHECK (building_count >= 0),
    vertex_count INTEGER NOT NULL CHECK (vertex_count >= 0),
    triangle_count INTEGER NOT NULL CHECK (triangle_count >= 0),
    primitive_count INTEGER NOT NULL CHECK (primitive_count >= 0),
    FOREIGN KEY (dataset_id) REFERENCES dataset_metadata(dataset_id),
    CHECK (tile_min_x <= tile_max_x),
    CHECK (tile_min_y <= tile_max_y)
);

CREATE TABLE buildings (
    building_id TEXT PRIMARY KEY,
    canonical_building_id TEXT NOT NULL UNIQUE,
    gml_id TEXT,
    id_source TEXT NOT NULL,
    id_is_synthetic INTEGER NOT NULL CHECK (id_is_synthetic IN (0, 1)),
    source_file_id INTEGER,
    tile_id TEXT NOT NULL,
    local_feature_id INTEGER NOT NULL CHECK (local_feature_id >= 0 AND local_feature_id <= 65534),
    lod_used INTEGER NOT NULL,
    lod_generated INTEGER NOT NULL CHECK (lod_generated IN (0, 1)),
    measured_height REAL,
    min_height REAL,
    max_height REAL,
    centroid_x REAL NOT NULL,
    centroid_y REAL NOT NULL,
    centroid_lon REAL,
    centroid_lat REAL,
    footprint_quality TEXT NOT NULL,
    attributes_json TEXT,
    footprint BLOB,
    FOREIGN KEY (source_file_id) REFERENCES source_files(source_file_id),
    FOREIGN KEY (tile_id) REFERENCES tiles(tile_id)
);

CREATE TABLE building_parts (
    building_part_id TEXT PRIMARY KEY,
    building_id TEXT NOT NULL,
    gml_id TEXT,
    FOREIGN KEY (building_id) REFERENCES buildings(building_id)
);

CREATE TABLE building_attributes (
    id INTEGER PRIMARY KEY,
    building_id TEXT NOT NULL,
    namespace_uri TEXT NOT NULL,
    attribute_path TEXT NOT NULL,
    attribute_key TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    value_type TEXT NOT NULL,
    value_text TEXT,
    value_integer INTEGER,
    value_real REAL,
    value_boolean INTEGER CHECK (value_boolean IN (0, 1)),
    value_datetime TEXT,
    uom TEXT,
    code_space TEXT,
    nil_reason TEXT,
    FOREIGN KEY (building_id) REFERENCES buildings(building_id),
    UNIQUE (building_id, namespace_uri, attribute_path, ordinal)
);

CREATE TABLE tile_features (
    tile_id TEXT NOT NULL,
    local_feature_id INTEGER NOT NULL CHECK (local_feature_id >= 0 AND local_feature_id <= 65534),
    building_id TEXT NOT NULL,
    building_part_id TEXT,
    PRIMARY KEY (tile_id, local_feature_id),
    FOREIGN KEY (tile_id) REFERENCES tiles(tile_id),
    FOREIGN KEY (building_id) REFERENCES buildings(building_id),
    FOREIGN KEY (building_part_id) REFERENCES building_parts(building_part_id)
);

CREATE TABLE conversion_issues (
    id INTEGER PRIMARY KEY,
    source_file_id INTEGER,
    building_id TEXT,
    gml_id TEXT,
    severity TEXT NOT NULL CHECK (severity IN ('trace', 'debug', 'info', 'warn', 'error', 'fatal')),
    error_code TEXT NOT NULL,
    message TEXT NOT NULL,
    element_path TEXT,
    repaired INTEGER NOT NULL DEFAULT 0 CHECK (repaired IN (0, 1)),
    exclusion_reason TEXT,
    occurred_at TEXT NOT NULL,
    FOREIGN KEY (source_file_id) REFERENCES source_files(source_file_id),
    FOREIGN KEY (building_id) REFERENCES buildings(building_id)
);

CREATE INDEX idx_buildings_gml_id ON buildings(gml_id);
CREATE INDEX idx_buildings_tile_feature ON buildings(tile_id, local_feature_id);
CREATE INDEX idx_building_parts_building_id ON building_parts(building_id);
CREATE INDEX idx_building_attributes_key_building ON building_attributes(attribute_key, building_id);
CREATE INDEX idx_tile_features_building_id ON tile_features(building_id);
CREATE INDEX idx_conversion_issues_building_id ON conversion_issues(building_id);
CREATE INDEX idx_conversion_issues_source_file_id ON conversion_issues(source_file_id);
