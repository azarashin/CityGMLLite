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
/// A terrain triangle with per-vertex UVs and a tile-local feature identity.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainTriangle {
    pub positions: [citymodel_coordinate::Point3; 3],
    pub uvs: [(f64, f64); 3],
    pub feature_id: String,
    pub texture_index: usize,
}
/// An image embedded in a terrain GLB.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerrainTexture {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}
/// Input for a self-contained, textured terrain GLB.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainGlbInput {
    pub tile_id: String,
    pub generation_id: String,
    pub triangles: Vec<TerrainTriangle>,
    pub feature_ids: BTreeMap<String, u16>,
    pub textures: Vec<TerrainTexture>,
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
    InvalidTexture,
}

/// Produces a self-contained textured terrain GLB. Images are stored in GLB
/// buffer views; no external URI is emitted.
///
/// # Errors
///
/// Returns an error for missing feature identities, unsupported textures, or a
/// GLB that cannot be represented within the format's integer limits.
#[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
pub fn write_terrain_glb(input: &TerrainGlbInput) -> Result<GlbAsset, GlbError> {
    if input.textures.is_empty() || input.triangles.is_empty() {
        return Err(GlbError::InvalidTexture);
    }
    if input.textures.iter().any(|texture| {
        texture.bytes.is_empty()
            || !matches!(texture.mime_type.as_str(), "image/png" | "image/jpeg")
    }) {
        return Err(GlbError::InvalidTexture);
    }
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut feature_ids = Vec::new();
    let mut indices = vec![Vec::<u32>::new(); input.textures.len()];
    for triangle in &input.triangles {
        let feature = input
            .feature_ids
            .get(&triangle.feature_id)
            .copied()
            .ok_or_else(|| GlbError::MissingFeatureId(triangle.feature_id.clone()))?;
        let Some(texture_indices) = indices.get_mut(triangle.texture_index) else {
            return Err(GlbError::InvalidTexture);
        };
        let base = u32::try_from(feature_ids.len()).map_err(|_| GlbError::InvalidGlb)?;
        let normal = terrain_normal(triangle.positions);
        for (point, uv) in triangle.positions.into_iter().zip(triangle.uvs) {
            positions.extend([point.x as f32, point.y as f32, point.z as f32]);
            normals.extend(normal);
            uvs.extend([uv.0 as f32, uv.1 as f32]);
            feature_ids.push(feature);
        }
        texture_indices.extend([base, base + 1, base + 2]);
    }
    let mut binary = Vec::new();
    let position_offset = append_f32(&mut binary, &positions);
    let normal_offset = append_f32(&mut binary, &normals);
    let uv_offset = append_f32(&mut binary, &uvs);
    let feature_offset = append_u16(&mut binary, &feature_ids);
    let mut index_offsets = Vec::new();
    for values in &indices {
        index_offsets.push((append_u32(&mut binary, values), values.len() * 4));
    }
    let mut image_offsets = Vec::new();
    for texture in &input.textures {
        pad(&mut binary);
        let offset = binary.len();
        binary.extend(&texture.bytes);
        image_offsets.push((offset, texture.bytes.len()));
    }
    pad(&mut binary);
    let mut buffer_views = vec![
        format!(
            r#"{{"buffer":0,"byteOffset":{position_offset},"byteLength":{},"target":34962}}"#,
            positions.len() * 4
        ),
        format!(
            r#"{{"buffer":0,"byteOffset":{normal_offset},"byteLength":{},"target":34962}}"#,
            normals.len() * 4
        ),
        format!(
            r#"{{"buffer":0,"byteOffset":{uv_offset},"byteLength":{},"target":34962}}"#,
            uvs.len() * 4
        ),
        format!(
            r#"{{"buffer":0,"byteOffset":{feature_offset},"byteLength":{},"target":34962}}"#,
            feature_ids.len() * 2
        ),
    ];
    buffer_views.extend(index_offsets.iter().map(|(offset, length)| {
        format!(r#"{{"buffer":0,"byteOffset":{offset},"byteLength":{length},"target":34963}}"#)
    }));
    buffer_views.extend(image_offsets.iter().map(|(offset, length)| {
        format!(r#"{{"buffer":0,"byteOffset":{offset},"byteLength":{length}}}"#)
    }));
    let primitive_json = indices.iter().enumerate().filter(|(_, values)| !values.is_empty()).map(|(index, _)| {
        format!(r#"{{"attributes":{{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2,"_FEATURE_ID_0":3}},"indices":{},"material":{},"mode":4}}"#, 4 + index, index)
    }).collect::<Vec<_>>().join(",");
    let images = input
        .textures
        .iter()
        .enumerate()
        .map(|(index, texture)| {
            format!(
                r#"{{"bufferView":{},"mimeType":"{}"}}"#,
                4 + indices.len() + index,
                texture.mime_type
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let textures = (0..input.textures.len())
        .map(|index| format!(r#"{{"source":{index}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let materials = (0..input.textures.len()).map(|index| format!(r#"{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":{index}}},"metallicFactor":0.0,"roughnessFactor":1.0}}}}"#)).collect::<Vec<_>>().join(",");
    let json = format!(
        r#"{{"asset":{{"version":"2.0","generator":"CityGMLLite","extras":{{"schemaVersion":"{}","tileId":"{}","generationId":"{}"}}}},"buffers":[{{"byteLength":{}}}],"bufferViews":[{}],"accessors":[{{"bufferView":0,"componentType":5126,"count":{},"type":"VEC3"}},{{"bufferView":1,"componentType":5126,"count":{},"type":"VEC3"}},{{"bufferView":2,"componentType":5126,"count":{},"type":"VEC2"}},{{"bufferView":3,"componentType":5123,"normalized":false,"count":{},"type":"SCALAR"}}],"images":[{}],"textures":[{}],"materials":[{}],"meshes":[{{"primitives":[{}]}}],"nodes":[{{"mesh":0}}],"scenes":[{{"nodes":[0]}}],"scene":0}}"#,
        contract_schema_version(),
        input.tile_id,
        input.generation_id,
        binary.len(),
        buffer_views.join(","),
        feature_ids.len(),
        feature_ids.len(),
        feature_ids.len(),
        feature_ids.len(),
        images,
        textures,
        materials,
        primitive_json
    );
    let mut json = json.into_bytes();
    pad(&mut json);
    let total = 12 + 8 + json.len() + 8 + binary.len();
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(
        &(u32::try_from(total).map_err(|_| GlbError::InvalidGlb)?).to_le_bytes(),
    );
    bytes.extend_from_slice(
        &(u32::try_from(json.len()).map_err(|_| GlbError::InvalidGlb)?).to_le_bytes(),
    );
    bytes.extend_from_slice(b"JSON");
    bytes.extend(json);
    bytes.extend_from_slice(
        &(u32::try_from(binary.len()).map_err(|_| GlbError::InvalidGlb)?).to_le_bytes(),
    );
    bytes.extend_from_slice(b"BIN\0");
    bytes.extend(binary);
    validate_glb(&bytes)?;
    Ok(GlbAsset {
        sha256: hex(Sha256::digest(&bytes)),
        bytes,
    })
}

fn terrain_normal(positions: [citymodel_coordinate::Point3; 3]) -> [f32; 3] {
    normal(&Triangle {
        positions,
        building_id: String::new(),
        building_part_id: None,
    })
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

    #[test]
    fn embeds_terrain_texture_and_uvs() {
        let positions = [
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
        ];
        let asset = write_terrain_glb(&TerrainGlbInput {
            tile_id: "terrain-tile".into(),
            generation_id: "g".into(),
            triangles: vec![TerrainTriangle {
                positions,
                uvs: [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)],
                feature_id: "terrain-1".into(),
                texture_index: 0,
            }],
            feature_ids: BTreeMap::from([("terrain-1".into(), 0)]),
            textures: vec![TerrainTexture {
                mime_type: "image/png".into(),
                bytes: vec![137, 80, 78, 71],
            }],
        })
        .unwrap();
        assert!(asset.bytes.windows(10).any(|part| part == b"TEXCOORD_0"));
        assert!(asset.bytes.windows(9).any(|part| part == b"image/png"));
        validate_glb(&asset.bytes).unwrap();
    }
}
