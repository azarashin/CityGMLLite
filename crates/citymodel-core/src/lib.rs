//! Shared types and versioning contracts for `CityModel` conversion and runtime.

/// Canonical version information shared by generated datasets and runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractVersion {
    pub schema_version: &'static str,
    pub generator_version: &'static str,
    pub database_user_version: u32,
}

/// Current data-contract version.
pub const CURRENT_CONTRACT_VERSION: ContractVersion = ContractVersion {
    schema_version: "1.0.0",
    generator_version: "0.1.0-dev",
    database_user_version: 1,
};

/// Canonical machine-readable source for version values.
pub const CONTRACT_VERSION_JSON: &str = include_str!("../../../contracts/version.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_source_contains_current_versions() {
        assert!(CONTRACT_VERSION_JSON.contains("\"schemaVersion\": \"1.0.0\""));
        assert!(CONTRACT_VERSION_JSON.contains(CURRENT_CONTRACT_VERSION.generator_version));
        assert!(CONTRACT_VERSION_JSON.contains("\"databaseUserVersion\": 1"));
    }
}
