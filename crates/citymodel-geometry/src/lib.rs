//! LOD1 building geometry validation, normalization, and triangulation.

use citymodel_coordinate::Point3;

pub const MODULE_NAME: &str = "citymodel-geometry";
#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Lod {
    Lod0,
    Lod1,
    Lod2,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon {
    pub outer: Vec<Point3>,
    pub holes: Vec<Vec<Point3>>,
    pub lod: Lod,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GeometryDiagnostic {
    InvalidCoordinate,
    DegenerateRing,
    HoleNotTriangulated,
    ReorientedFace,
    Excluded,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FootprintQuality {
    Source,
    Derived,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Triangle {
    pub positions: [Point3; 3],
    pub building_id: String,
    pub building_part_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedGeometry {
    pub lod: Lod,
    pub triangles: Vec<Triangle>,
    pub footprint: Vec<Point3>,
    pub footprint_quality: FootprintQuality,
    pub diagnostics: Vec<GeometryDiagnostic>,
}

/// Selects the requested LOD, falling back to the highest available lower LOD.
#[must_use]
pub fn select_lod(polygons: &[Polygon], requested: Lod) -> Option<(Lod, Vec<&Polygon>)> {
    [requested, Lod::Lod1, Lod::Lod0]
        .into_iter()
        .find_map(|lod| {
            let selected: Vec<_> = polygons
                .iter()
                .filter(|polygon| polygon.lod == lod)
                .collect();
            (!selected.is_empty()).then_some((lod, selected))
        })
}

/// Validates and triangulates LOD geometry without sharing vertices across buildings.
#[must_use]
#[allow(clippy::needless_pass_by_value)]
pub fn normalize_building_geometry(
    building_id: impl Into<String>,
    building_part_id: Option<String>,
    polygons: &[Polygon],
    requested_lod: Lod,
) -> NormalizedGeometry {
    let building_id = building_id.into();
    let Some((lod, polygons)) = select_lod(polygons, requested_lod) else {
        return NormalizedGeometry {
            lod: requested_lod,
            triangles: Vec::new(),
            footprint: Vec::new(),
            footprint_quality: FootprintQuality::Unavailable,
            diagnostics: vec![GeometryDiagnostic::Excluded],
        };
    };
    let mut result = NormalizedGeometry {
        lod,
        triangles: Vec::new(),
        footprint: Vec::new(),
        footprint_quality: FootprintQuality::Unavailable,
        diagnostics: Vec::new(),
    };
    for polygon in polygons {
        let Some(mut ring) = normalize_ring(&polygon.outer, &mut result.diagnostics) else {
            continue;
        };
        if !polygon.holes.is_empty() {
            result
                .diagnostics
                .push(GeometryDiagnostic::HoleNotTriangulated);
        }
        if signed_area_xy(&ring) < 0.0 {
            ring.reverse();
            result.diagnostics.push(GeometryDiagnostic::ReorientedFace);
        }
        if result.footprint.is_empty() {
            result.footprint.clone_from(&ring);
            result.footprint_quality = FootprintQuality::Source;
        }
        for index in 1..ring.len() - 1 {
            result.triangles.push(Triangle {
                positions: [ring[0], ring[index], ring[index + 1]],
                building_id: building_id.clone(),
                building_part_id: building_part_id.clone(),
            });
        }
    }
    if result.triangles.is_empty() {
        result.diagnostics.push(GeometryDiagnostic::Excluded);
    }
    result
}

fn normalize_ring(
    input: &[Point3],
    diagnostics: &mut Vec<GeometryDiagnostic>,
) -> Option<Vec<Point3>> {
    if input
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        diagnostics.push(GeometryDiagnostic::InvalidCoordinate);
        return None;
    }
    let mut ring = input.to_vec();
    if ring.first() == ring.last() {
        ring.pop();
    }
    if ring.len() < 3 || signed_area_xy(&ring).abs() <= f64::EPSILON {
        diagnostics.push(GeometryDiagnostic::DegenerateRing);
        return None;
    }
    Some(ring)
}
fn signed_area_xy(ring: &[Point3]) -> f64 {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .map(|(left, right)| left.x * right.y - right.x * left.y)
        .sum::<f64>()
        / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn point(x: f64, y: f64) -> Point3 {
        Point3 { x, y, z: 0.0 }
    }
    #[test]
    fn lod1_triangles_preserve_identity_and_do_not_share_vertices() {
        let polygon = Polygon {
            outer: vec![
                point(0.0, 0.0),
                point(1.0, 0.0),
                point(1.0, 1.0),
                point(0.0, 1.0),
                point(0.0, 0.0),
            ],
            holes: Vec::new(),
            lod: Lod::Lod1,
        };
        let geometry =
            normalize_building_geometry("b-1", Some("part-1".to_owned()), &[polygon], Lod::Lod1);
        assert_eq!(geometry.triangles.len(), 2);
        assert!(
            geometry
                .triangles
                .iter()
                .all(|triangle| triangle.building_id == "b-1"
                    && triangle.building_part_id.as_deref() == Some("part-1"))
        );
    }
    #[test]
    fn invalid_geometry_is_diagnosed_without_panicking() {
        let polygon = Polygon {
            outer: vec![point(0.0, 0.0), point(0.0, 0.0), point(0.0, 0.0)],
            holes: Vec::new(),
            lod: Lod::Lod1,
        };
        assert!(
            normalize_building_geometry("b", None, &[polygon], Lod::Lod1)
                .diagnostics
                .contains(&GeometryDiagnostic::DegenerateRing)
        );
    }
}
