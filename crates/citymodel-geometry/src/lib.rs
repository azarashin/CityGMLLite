//! Geometry extraction, validation, and triangulation boundary.

pub const MODULE_NAME: &str = "citymodel-geometry";

#[must_use]
pub fn contract_schema_version() -> u32 {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}
