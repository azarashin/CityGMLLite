-- Add a feature-type-independent identity, attribute, and tile mapping layer.
--
-- Building tables remain the compatibility API for v1 consumers. Every
-- building written by the converter is also registered in these tables.

CREATE TABLE features (
    feature_id TEXT PRIMARY KEY,
    canonical_feature_id TEXT NOT NULL UNIQUE,
    feature_type TEXT NOT NULL CHECK (length(feature_type) > 0),
    gml_id TEXT,
    id_source TEXT NOT NULL,
    id_is_synthetic INTEGER NOT NULL CHECK (id_is_synthetic IN (0, 1)),
    source_file_id INTEGER,
    FOREIGN KEY (source_file_id) REFERENCES source_files(source_file_id)
);

CREATE TABLE feature_attributes (
    id INTEGER PRIMARY KEY,
    feature_id TEXT NOT NULL,
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
    FOREIGN KEY (feature_id) REFERENCES features(feature_id),
    UNIQUE (feature_id, namespace_uri, attribute_path, ordinal)
);

-- One tile may contain independently-addressable content for several types.
-- The legacy `tiles` row continues to describe the building artifact only.
CREATE TABLE tile_contents (
    tile_id TEXT NOT NULL,
    feature_type TEXT NOT NULL CHECK (length(feature_type) > 0),
    metadata_relative_path TEXT NOT NULL,
    metadata_sha256 TEXT NOT NULL CHECK (length(metadata_sha256) = 64),
    metadata_byte_length INTEGER NOT NULL CHECK (metadata_byte_length >= 0),
    glb_relative_path TEXT NOT NULL,
    glb_sha256 TEXT NOT NULL CHECK (length(glb_sha256) = 64),
    glb_byte_length INTEGER NOT NULL CHECK (glb_byte_length >= 0),
    PRIMARY KEY (tile_id, feature_type),
    FOREIGN KEY (tile_id) REFERENCES tiles(tile_id)
);

CREATE TABLE feature_tile_mappings (
    tile_id TEXT NOT NULL,
    feature_type TEXT NOT NULL CHECK (length(feature_type) > 0),
    local_feature_id INTEGER NOT NULL CHECK (local_feature_id >= 0 AND local_feature_id <= 65534),
    feature_id TEXT NOT NULL,
    PRIMARY KEY (tile_id, feature_type, local_feature_id),
    FOREIGN KEY (tile_id) REFERENCES tiles(tile_id),
    FOREIGN KEY (tile_id, feature_type) REFERENCES tile_contents(tile_id, feature_type),
    FOREIGN KEY (feature_id) REFERENCES features(feature_id)
);

CREATE INDEX idx_features_type ON features(feature_type);
CREATE INDEX idx_feature_attributes_key_feature ON feature_attributes(attribute_key, feature_id);
CREATE INDEX idx_feature_tile_mappings_feature ON feature_tile_mappings(feature_id);

INSERT INTO schema_migrations (version, applied_at, description)
VALUES (2, CURRENT_TIMESTAMP, 'Add common features and generic tile mappings');

PRAGMA user_version = 2;
