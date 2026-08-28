use serde_json::{Value, json};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Metadata emitted beside converter artifacts.
#[derive(Clone, Debug)]
pub struct TileMetadataInput<'a> {
    pub generation_id: &'a str,
    pub tile_id: &'a str,
    pub glb_path: &'a str,
    pub glb_sha256: &'a str,
    pub glb_byte_length: u64,
    pub building_ids: &'a [String],
    /// Type-neutral feature table. `buildingIds` remains for v1 readers.
    pub feature_type: &'a str,
    pub tile_bounds: [f64; 4],
    pub content_bounds: [f64; 6],
    pub projected_origin: [f64; 3],
    pub geographic_origin: [f64; 3],
    pub working_epsg: u32,
    pub vertex_count: usize,
    pub triangle_count: usize,
}

/// Fails closed when a metadata path can escape its dataset root.
pub fn safe_relative_path(path: &str) -> Result<&str, &'static str> {
    let value = Path::new(path);
    if value.is_absolute()
        || value.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err("metadata path must stay within the dataset root")
    } else {
        Ok(path)
    }
}

#[must_use]
pub fn tile_metadata_json(input: &TileMetadataInput<'_>) -> Value {
    json!({
        "schemaVersion":"1.0.0", "generationId":input.generation_id, "tileId":input.tile_id,
        "content":{"featureType":input.feature_type,"glb":input.glb_path,"sha256":input.glb_sha256,"byteLength":input.glb_byte_length},
        "origin":{"geographic":{"latitude":input.geographic_origin[0],"longitude":input.geographic_origin[1],"height":input.geographic_origin[2],"epsg":6668},"projected":{"x":input.projected_origin[0],"y":input.projected_origin[1],"z":input.projected_origin[2],"epsg":input.working_epsg}},
        "coordinateFrame":{"unit":"metre","handedness":"right","xAxis":"east","yAxis":"up","zAxis":"south","projectedToLocalMatrix":[1.0,0.0,0.0,-input.projected_origin[0],0.0,0.0,1.0,-input.projected_origin[2],0.0,-1.0,0.0,input.projected_origin[1],0.0,0.0,0.0,1.0]},
        "tileBounds":{"minX":input.tile_bounds[0],"minY":input.tile_bounds[1],"maxX":input.tile_bounds[2],"maxY":input.tile_bounds[3]},
        "contentBounds":{"minX":input.content_bounds[0],"minY":input.content_bounds[1],"minZ":input.content_bounds[2],"maxX":input.content_bounds[3],"maxY":input.content_bounds[4],"maxZ":input.content_bounds[5]},
        "features":{"semantic":"_FEATURE_ID_0","componentType":"UNSIGNED_SHORT","nullFeatureId":65535,"buildingIds":input.building_ids,"items":input.building_ids.iter().enumerate().map(|(local_feature_id, feature_id)| json!({"localFeatureId":local_feature_id,"featureId":feature_id,"featureType":input.feature_type})).collect::<Vec<_>>()},
        "statistics":{"buildingCount":input.building_ids.len(),"vertexCount":input.vertex_count,"triangleCount":input.triangle_count,"primitiveCount":1}
    })
}

/// Writes metadata atomically after validating its relative output path.
///
/// # Errors
///
/// Returns an error if the path is unsafe or the JSON cannot be written.
pub fn write_json_under(
    root: &Path,
    relative_path: &str,
    value: &Value,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    safe_relative_path(relative_path)?;
    let output = root.join(relative_path);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(value)?)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_dataset_escape() {
        assert!(safe_relative_path("../out.json").is_err());
        assert!(safe_relative_path("tiles/t.meta.json").is_ok());
    }
}
