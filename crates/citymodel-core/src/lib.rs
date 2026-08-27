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

/// Provenance of the value selected as a `BuildingID`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildingIdSource {
    Plateau,
    CustomAttribute,
    GmlId,
    Synthetic,
}

/// A stable building identifier and the information needed for diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildingIdentity {
    pub building_id: String,
    pub canonical_building_id: String,
    pub gml_id: Option<String>,
    pub id_source: BuildingIdSource,
    pub id_is_synthetic: bool,
    pub parent_building_id: Option<String>,
    pub building_part_id: Option<String>,
}

/// Raw identity candidates extracted from one `CityGML` feature.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BuildingIdentityInput<'a> {
    pub dataset_id: &'a str,
    pub plateau_id: Option<&'a str>,
    pub custom_id: Option<&'a str>,
    pub gml_id: Option<&'a str>,
    pub source_path: &'a str,
    pub feature_ordinal: u64,
    pub parent_building_id: Option<&'a str>,
}

/// Resolves the required `BuildingID` priority order without random state.
#[must_use]
pub fn resolve_building_identity(input: &BuildingIdentityInput<'_>) -> BuildingIdentity {
    let (building_id, id_source) = first_non_empty(input.plateau_id)
        .map(|value| (value.to_owned(), BuildingIdSource::Plateau))
        .or_else(|| {
            first_non_empty(input.custom_id)
                .map(|value| (value.to_owned(), BuildingIdSource::CustomAttribute))
        })
        .or_else(|| {
            first_non_empty(input.gml_id).map(|value| (value.to_owned(), BuildingIdSource::GmlId))
        })
        .unwrap_or_else(|| (synthetic_id(input), BuildingIdSource::Synthetic));
    let parent_building_id = first_non_empty(input.parent_building_id).map(str::to_owned);
    BuildingIdentity {
        canonical_building_id: format!("{}::{building_id}", input.dataset_id),
        building_part_id: parent_building_id.as_ref().map(|_| building_id.clone()),
        building_id,
        gml_id: input.gml_id.map(str::to_owned),
        id_is_synthetic: id_source == BuildingIdSource::Synthetic,
        id_source,
        parent_building_id,
    }
}

/// Tile-local mapping from deterministic `FeatureID` values to building IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileFeatureMap {
    pub building_ids: Vec<String>,
}

/// An invalid duplicate ID that must never be overwritten implicitly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplicateBuildingId {
    pub building_id: String,
}

impl TileFeatureMap {
    /// Builds a lexicographically stable feature table and rejects duplicate IDs.
    ///
    /// # Errors
    ///
    /// Returns the duplicate ID instead of silently selecting one feature.
    pub fn from_building_ids(
        ids: impl IntoIterator<Item = String>,
    ) -> Result<Self, DuplicateBuildingId> {
        let mut building_ids: Vec<_> = ids.into_iter().collect();
        building_ids.sort();
        if let Some(duplicate) = building_ids.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(DuplicateBuildingId {
                building_id: duplicate[0].clone(),
            });
        }
        Ok(Self { building_ids })
    }

    /// Returns the tile-local ID allocated for a building.
    #[must_use]
    pub fn feature_id(&self, building_id: &str) -> Option<u16> {
        self.building_ids
            .binary_search_by(|candidate| candidate.as_str().cmp(building_id))
            .ok()
            .and_then(|index| u16::try_from(index).ok())
    }
}

fn first_non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|candidate| !candidate.trim().is_empty())
}

fn synthetic_id(input: &BuildingIdentityInput<'_>) -> String {
    let text = format!(
        "{}\u{1f}{}\u{1f}{}",
        input.dataset_id, input.source_path, input.feature_ordinal
    );
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |state, byte| {
        (state ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("synthetic-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_source_contains_current_versions() {
        assert!(CONTRACT_VERSION_JSON.contains("\"schemaVersion\": \"1.0.0\""));
        assert!(CONTRACT_VERSION_JSON.contains(CURRENT_CONTRACT_VERSION.generator_version));
        assert!(CONTRACT_VERSION_JSON.contains("\"databaseUserVersion\": 1"));
    }

    #[test]
    fn resolves_ids_by_priority_and_marks_synthetic_values() {
        let input = BuildingIdentityInput {
            dataset_id: "13100",
            plateau_id: Some("plateau"),
            custom_id: Some("custom"),
            gml_id: Some("gml"),
            source_path: "bldg.gml",
            feature_ordinal: 3,
            parent_building_id: None,
        };
        assert_eq!(resolve_building_identity(&input).building_id, "plateau");
        let synthetic = resolve_building_identity(&BuildingIdentityInput {
            plateau_id: None,
            custom_id: None,
            gml_id: None,
            ..input
        });
        assert!(synthetic.id_is_synthetic);
        assert_eq!(synthetic.id_source, BuildingIdSource::Synthetic);
    }

    #[test]
    fn feature_ids_are_sorted_and_duplicates_are_rejected() {
        let map = TileFeatureMap::from_building_ids(["b".to_owned(), "a".to_owned()]).unwrap();
        assert_eq!(map.feature_id("a"), Some(0));
        assert_eq!(map.feature_id("b"), Some(1));
        assert_eq!(
            TileFeatureMap::from_building_ids(["a".to_owned(), "a".to_owned()])
                .unwrap_err()
                .building_id,
            "a"
        );
    }
}
