//! `SQLite` and `SpatiaLite` writer boundary.

pub const MODULE_NAME: &str = "citymodel-spatialite";

#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}
