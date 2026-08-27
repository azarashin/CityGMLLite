//! Deterministic projected-grid tile assignment.

pub const MODULE_NAME: &str = "citymodel-tiling";
#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}
pub const DEFAULT_TILE_SIZE_METERS: f64 = 500.0;
pub const MAX_FEATURE_ID: usize = 65_535;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TileId {
    pub x: i64,
    pub y: i64,
    pub level: u8,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileLimits {
    pub max_features: usize,
    pub max_vertices: usize,
    pub max_triangles: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TileAssignment {
    pub building_id: String,
    pub representative_x: f64,
    pub representative_y: f64,
    pub vertices: usize,
    pub triangles: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub id: TileId,
    pub bounds: Bounds,
    pub content_bounds: Option<Bounds>,
    pub buildings: Vec<TileAssignment>,
    pub neighbors: Vec<TileId>,
}

impl Default for TileLimits {
    fn default() -> Self {
        Self {
            max_features: MAX_FEATURE_ID,
            max_vertices: 2_000_000,
            max_triangles: 2_000_000,
        }
    }
}

/// Assigns buildings by representative point without clipping their geometry.
///
/// # Panics
///
/// Panics when `tile_size` is zero, negative, or non-finite.
#[must_use]
pub fn assign_tiles(
    mut buildings: Vec<TileAssignment>,
    tile_size: f64,
    limits: TileLimits,
) -> Vec<Tile> {
    assert!(
        tile_size.is_finite() && tile_size > 0.0,
        "tile size must be positive"
    );
    buildings.sort_by(|left, right| left.building_id.cmp(&right.building_id));
    let mut tiles = std::collections::BTreeMap::<TileId, Vec<TileAssignment>>::new();
    for building in buildings {
        let id = tile_for_point(
            building.representative_x,
            building.representative_y,
            tile_size,
        );
        tiles.entry(id).or_default().push(building);
    }
    let mut output = Vec::new();
    for (id, buildings) in tiles {
        subdivide(id, buildings, tile_size, limits, &mut output);
    }
    output.sort_by_key(|left| left.id);
    let ids: std::collections::BTreeSet<_> = output.iter().map(|tile| tile.id).collect();
    for tile in &mut output {
        tile.neighbors = cardinal_neighbors(tile.id)
            .into_iter()
            .filter(|neighbor| ids.contains(neighbor))
            .collect();
    }
    output
}

#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn tile_for_point(x: f64, y: f64, tile_size: f64) -> TileId {
    TileId {
        x: (x / tile_size).floor() as i64,
        y: (y / tile_size).floor() as i64,
        level: 0,
    }
}

#[allow(clippy::cast_possible_truncation)]
fn subdivide(
    id: TileId,
    buildings: Vec<TileAssignment>,
    tile_size: f64,
    limits: TileLimits,
    output: &mut Vec<Tile>,
) {
    let vertices: usize = buildings.iter().map(|building| building.vertices).sum();
    let triangles: usize = buildings.iter().map(|building| building.triangles).sum();
    if buildings.len() <= limits.max_features
        && vertices <= limits.max_vertices
        && triangles <= limits.max_triangles
    {
        output.push(make_tile(id, buildings, tile_size));
        return;
    }
    let child_size = tile_size / 2_f64.powi(i32::from(id.level) + 1);
    let mut children = std::collections::BTreeMap::<TileId, Vec<TileAssignment>>::new();
    for building in buildings {
        let child = TileId {
            x: (building.representative_x / child_size).floor() as i64,
            y: (building.representative_y / child_size).floor() as i64,
            level: id.level + 1,
        };
        children.entry(child).or_default().push(building);
    }
    for (child, buildings) in children {
        subdivide(child, buildings, tile_size, limits, output);
    }
}

#[allow(clippy::cast_precision_loss)]
fn make_tile(id: TileId, buildings: Vec<TileAssignment>, base_size: f64) -> Tile {
    let size = base_size / 2_f64.powi(i32::from(id.level));
    let bounds = Bounds {
        min_x: id.x as f64 * size,
        min_y: id.y as f64 * size,
        max_x: (id.x + 1) as f64 * size,
        max_y: (id.y + 1) as f64 * size,
    };
    let content_bounds = buildings.iter().fold(None::<Bounds>, |current, building| {
        Some(match current {
            Some(bounds) => Bounds {
                min_x: bounds.min_x.min(building.representative_x),
                min_y: bounds.min_y.min(building.representative_y),
                max_x: bounds.max_x.max(building.representative_x),
                max_y: bounds.max_y.max(building.representative_y),
            },
            None => Bounds {
                min_x: building.representative_x,
                min_y: building.representative_y,
                max_x: building.representative_x,
                max_y: building.representative_y,
            },
        })
    });
    Tile {
        id,
        bounds,
        content_bounds,
        buildings,
        neighbors: Vec::new(),
    }
}
fn cardinal_neighbors(id: TileId) -> [TileId; 4] {
    [
        TileId { x: id.x - 1, ..id },
        TileId { x: id.x + 1, ..id },
        TileId { y: id.y - 1, ..id },
        TileId { y: id.y + 1, ..id },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn representative_point_assigns_a_stable_500m_tile() {
        let tiles = assign_tiles(
            vec![TileAssignment {
                building_id: "b".to_owned(),
                representative_x: 500.0,
                representative_y: 0.0,
                vertices: 3,
                triangles: 1,
            }],
            DEFAULT_TILE_SIZE_METERS,
            TileLimits::default(),
        );
        assert_eq!(
            tiles[0].id,
            TileId {
                x: 1,
                y: 0,
                level: 0
            }
        );
    }
}
