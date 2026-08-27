//! Output validation, diagnostics, and security gates.

use std::path::{Component, Path};

pub const MODULE_NAME: &str = "citymodel-validation";
#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Warning,
    Error,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
}

/// Validates a relative artifact path and rejects absolute or traversal forms.
#[must_use]
pub fn validate_relative_path(path: impl AsRef<Path>) -> Option<ValidationIssue> {
    let path = path.as_ref();
    (path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }))
    .then(|| ValidationIssue {
        severity: Severity::Error,
        code: "PATH_ESCAPE",
        message: format!("unsafe artifact path: {}", path.display()),
    })
}
/// Validates generation consistency and GLB structural integrity before publication.
#[must_use]
pub fn validate_tile_artifact(
    expected_generation: &str,
    actual_generation: &str,
    relative_path: impl AsRef<Path>,
    glb: &[u8],
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if expected_generation != actual_generation {
        issues.push(ValidationIssue {
            severity: Severity::Error,
            code: "GENERATION_MISMATCH",
            message: "artifact generationId differs from manifest".to_owned(),
        });
    }
    if let Some(issue) = validate_relative_path(relative_path) {
        issues.push(issue);
    }
    if citymodel_gltf::validate_glb(glb).is_err() {
        issues.push(ValidationIssue {
            severity: Severity::Error,
            code: "INVALID_GLB",
            message: "GLB header or chunk boundaries are invalid".to_owned(),
        });
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_generation_and_path_escape() {
        let issues = validate_tile_artifact("a", "b", "../tile.glb", b"not-glb");
        assert_eq!(issues.len(), 3);
        assert!(issues.iter().all(|issue| issue.severity == Severity::Error));
    }
}
