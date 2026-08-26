//! Output validation and diagnostics boundary.

pub const MODULE_NAME: &str = "citymodel-validation";

#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}
