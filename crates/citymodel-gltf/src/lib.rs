//! GLB 2.0 writer with tile-local building Feature IDs.

use citymodel_geometry::Triangle;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Write as _};

pub const MODULE_NAME: &str = "citymodel-gltf";
#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}

#[derive(Clone, Debug, PartialEq)]
pub struct TileGlbInput {
    pub tile_id: String,
    pub generation_id: String,
    pub triangles: Vec<Triangle>,
    pub feature_ids: BTreeMap<String, u16>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlbAsset {
    pub bytes: Vec<u8>,
    pub sha256: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GlbError {
    MissingFeatureId(String),
    InvalidGlb,
}

/// Produces one combined glTF 2.0 binary mesh per tile.
///
/// # Errors
///
/// Returns an error when a triangle has no tile-local Feature ID mapping.
#[allow(clippy::cast_possible_truncation)]
pub fn write_tile_glb(input: &TileGlbInput) -> Result<GlbAsset, GlbError> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut feature_ids = Vec::new();
    let mut indices = Vec::new();
    for triangle in &input.triangles {
        let feature = input
            .feature_ids
            .get(&triangle.building_id)
            .copied()
            .ok_or_else(|| GlbError::MissingFeatureId(triangle.building_id.clone()))?;
        let base = u32::try_from(feature_ids.len()).map_err(|_| GlbError::InvalidGlb)?;
        let normal = normal(triangle);
        for point in triangle.positions {
            positions.extend([point.x as f32, point.y as f32, point.z as f32]);
            normals.extend(normal);
            feature_ids.push(feature);
        }
        indices.extend([base, base + 1, base + 2]);
    }
    let mut binary = Vec::new();
    let position_offset = append_f32(&mut binary, &positions);
    let normal_offset = append_f32(&mut binary, &normals);
    let feature_offset = append_u16(&mut binary, &feature_ids);
    let index_offset = append_u32(&mut binary, &indices);
    pad(&mut binary);
    let json = format!(
        r#"{{"asset":{{"version":"2.0","generator":"CityGMLLite","extras":{{"schemaVersion":"{}","tileId":"{}","generationId":"{}"}}}},"buffers":[{{"byteLength":{}}}],"bufferViews":[{{"buffer":0,"byteOffset":{},"byteLength":{},"target":34962}},{{"buffer":0,"byteOffset":{},"byteLength":{},"target":34962}},{{"buffer":0,"byteOffset":{},"byteLength":{},"target":34962}},{{"buffer":0,"byteOffset":{},"byteLength":{},"target":34963}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":{},"type":"VEC3"}},{{"bufferView":1,"componentType":5126,"count":{},"type":"VEC3"}},{{"bufferView":2,"componentType":5123,"normalized":false,"count":{},"type":"SCALAR"}},{{"bufferView":3,"componentType":5125,"count":{},"type":"SCALAR"}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1,"_FEATURE_ID_0":2}},"indices":3,"mode":4}}]}}],"nodes":[{{"mesh":0}}],"scenes":[{{"nodes":[0]}}],"scene":0}}"#,
        contract_schema_version(),
        input.tile_id,
        input.generation_id,
        binary.len(),
        position_offset,
        positions.len() * 4,
        normal_offset,
        normals.len() * 4,
        feature_offset,
        feature_ids.len() * 2,
        index_offset,
        indices.len() * 4,
        feature_ids.len(),
        feature_ids.len(),
        feature_ids.len(),
        indices.len()
    );
    let mut json = json.into_bytes();
    pad(&mut json);
    let total = 12 + 8 + json.len() + 8 + binary.len();
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&(total as u32).to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"JSON");
    bytes.extend(json);
    bytes.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"BIN\0");
    bytes.extend(binary);
    validate_glb(&bytes)?;
    Ok(GlbAsset {
        sha256: hex(Sha256::digest(&bytes)),
        bytes,
    })
}

/// Performs minimal GLB header and chunk-boundary validation.
///
/// # Errors
///
/// Returns `InvalidGlb` for malformed headers or chunk lengths.
pub fn validate_glb(bytes: &[u8]) -> Result<(), GlbError> {
    if bytes.len() < 20
        || &bytes[..4] != b"glTF"
        || u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| GlbError::InvalidGlb)?) as usize
            != bytes.len()
    {
        return Err(GlbError::InvalidGlb);
    }
    Ok(())
}
fn append_f32(output: &mut Vec<u8>, values: &[f32]) -> usize {
    let offset = output.len();
    for value in values {
        output.extend(value.to_le_bytes());
    }
    offset
}
fn append_u16(output: &mut Vec<u8>, values: &[u16]) -> usize {
    let offset = output.len();
    for value in values {
        output.extend(value.to_le_bytes());
    }
    offset
}
fn append_u32(output: &mut Vec<u8>, values: &[u32]) -> usize {
    let offset = output.len();
    for value in values {
        output.extend(value.to_le_bytes());
    }
    offset
}
fn pad(values: &mut Vec<u8>) {
    while values.len() % 4 != 0 {
        values.push(b' ');
    }
}
#[allow(clippy::many_single_char_names, clippy::cast_possible_truncation)]
fn normal(triangle: &Triangle) -> [f32; 3] {
    let [a, b, c] = triangle.positions;
    let u = (b.x - a.x, b.y - a.y, b.z - a.z);
    let v = (c.x - a.x, c.y - a.y, c.z - a.z);
    let n = (
        u.1 * v.2 - u.2 * v.1,
        u.2 * v.0 - u.0 * v.2,
        u.0 * v.1 - u.1 * v.0,
    );
    let length = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt();
    if length == 0.0 {
        [0.0, 0.0, 1.0]
    } else {
        [
            (n.0 / length) as f32,
            (n.1 / length) as f32,
            (n.2 / length) as f32,
        ]
    }
}
fn hex(digest: impl IntoIterator<Item = u8>) -> String {
    let mut value = String::with_capacity(64);
    for byte in digest {
        write!(value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use citymodel_coordinate::Point3;
    #[test]
    fn emits_valid_combined_glb() {
        let triangle = Triangle {
            positions: [
                Point3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                Point3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                Point3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            ],
            building_id: "b".into(),
            building_part_id: None,
        };
        let asset = write_tile_glb(&TileGlbInput {
            tile_id: "t".into(),
            generation_id: "g".into(),
            triangles: vec![triangle],
            feature_ids: BTreeMap::from([("b".into(), 0)]),
        })
        .unwrap();
        assert!(
            asset
                .bytes
                .windows(13)
                .any(|chunk| chunk == b"_FEATURE_ID_0")
        );
        assert_eq!(asset.sha256.len(), 64);
    }
}
