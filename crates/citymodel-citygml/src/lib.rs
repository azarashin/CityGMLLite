//! Streaming `CityGML` input boundary.

/// Name used by diagnostics to identify this module.
pub const MODULE_NAME: &str = "citymodel-citygml";

/// Exposes the data-contract version consumed by future parser output.
#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}
