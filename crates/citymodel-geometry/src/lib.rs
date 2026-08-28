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
    NonPlanarRing,
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
        // Preserve the existing horizontal-face convention (upward-facing winding),
        // but retain source winding for vertical faces. A vertical face has no useful
        // XY winding, and its source ring order carries the Solid shell orientation.
        let normal = newell_normal(&ring);
        if normal.2.abs() >= normal.0.abs().max(normal.1.abs()) && signed_area_xy(&ring) < 0.0 {
            ring.reverse();
            result.diagnostics.push(GeometryDiagnostic::ReorientedFace);
        }
        if result.footprint.is_empty() {
            result.footprint.clone_from(&ring);
            result.footprint_quality = FootprintQuality::Source;
        }
        let Some(triangles) = triangulate_ring(&ring) else {
            result.diagnostics.push(GeometryDiagnostic::DegenerateRing);
            continue;
        };
        for positions in triangles {
            result.triangles.push(Triangle {
                positions,
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
    if ring.len() < 3 {
        diagnostics.push(GeometryDiagnostic::DegenerateRing);
        return None;
    }
    let normal = newell_normal(&ring);
    let normal_length = length(normal);
    if normal_length <= f64::EPSILON {
        diagnostics.push(GeometryDiagnostic::DegenerateRing);
        return None;
    }
    let origin = ring[0];
    let extent = ring
        .iter()
        .map(|point| distance(origin, *point))
        .fold(1.0_f64, f64::max);
    if ring
        .iter()
        .skip(1)
        .any(|point| dot(subtract(*point, origin), normal).abs() / normal_length > extent * 1.0e-8)
    {
        diagnostics.push(GeometryDiagnostic::NonPlanarRing);
        return None;
    }
    Some(ring)
}

/// Triangulates a simple, planar 3D ring by projecting it onto its dominant plane.
/// The original coordinates and winding are retained in the returned triangles.
fn triangulate_ring(ring: &[Point3]) -> Option<Vec<[Point3; 3]>> {
    let normal = newell_normal(ring);
    let projected: Vec<_> = ring
        .iter()
        .map(|point| project_to_dominant_plane(*point, normal))
        .collect();
    let orientation = signed_area_2d(&projected);
    if orientation.abs() <= f64::EPSILON {
        return None;
    }
    let winding = orientation.signum();
    let mut remaining: Vec<_> = (0..ring.len()).collect();
    let mut triangles = Vec::with_capacity(ring.len() - 2);
    while remaining.len() > 3 {
        let mut ear_found = false;
        for position in 0..remaining.len() {
            let previous = remaining[(position + remaining.len() - 1) % remaining.len()];
            let current = remaining[position];
            let next = remaining[(position + 1) % remaining.len()];
            if !is_ear(previous, current, next, &remaining, &projected, winding) {
                continue;
            }
            triangles.push([ring[previous], ring[current], ring[next]]);
            remaining.remove(position);
            ear_found = true;
            break;
        }
        if !ear_found {
            return None;
        }
    }
    triangles.push([ring[remaining[0]], ring[remaining[1]], ring[remaining[2]]]);
    Some(triangles)
}

fn is_ear(
    previous: usize,
    current: usize,
    next: usize,
    remaining: &[usize],
    projected: &[(f64, f64)],
    winding: f64,
) -> bool {
    let a = projected[previous];
    let b = projected[current];
    let c = projected[next];
    if cross_2d(a, b, c) * winding <= f64::EPSILON {
        return false;
    }
    !remaining.iter().copied().any(|index| {
        index != previous
            && index != current
            && index != next
            && point_in_triangle(projected[index], a, b, c, winding)
    })
}

fn point_in_triangle(
    point: (f64, f64),
    a: (f64, f64),
    b: (f64, f64),
    c: (f64, f64),
    winding: f64,
) -> bool {
    cross_2d(a, b, point) * winding >= -f64::EPSILON
        && cross_2d(b, c, point) * winding >= -f64::EPSILON
        && cross_2d(c, a, point) * winding >= -f64::EPSILON
}

fn project_to_dominant_plane(point: Point3, normal: (f64, f64, f64)) -> (f64, f64) {
    if normal.0.abs() >= normal.1.abs().max(normal.2.abs()) {
        (point.y, point.z)
    } else if normal.1.abs() >= normal.2.abs() {
        (point.z, point.x)
    } else {
        (point.x, point.y)
    }
}

fn signed_area_2d(ring: &[(f64, f64)]) -> f64 {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .map(|(left, right)| left.0 * right.1 - right.0 * left.1)
        .sum::<f64>()
        / 2.0
}

fn cross_2d(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

fn newell_normal(ring: &[Point3]) -> (f64, f64, f64) {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .fold((0.0, 0.0, 0.0), |normal, (left, right)| {
            (
                normal.0 + (left.y - right.y) * (left.z + right.z),
                normal.1 + (left.z - right.z) * (left.x + right.x),
                normal.2 + (left.x - right.x) * (left.y + right.y),
            )
        })
}

fn subtract(left: Point3, right: Point3) -> (f64, f64, f64) {
    (left.x - right.x, left.y - right.y, left.z - right.z)
}

fn dot(left: (f64, f64, f64), right: (f64, f64, f64)) -> f64 {
    left.0 * right.0 + left.1 * right.1 + left.2 * right.2
}

fn length(vector: (f64, f64, f64)) -> f64 {
    dot(vector, vector).sqrt()
}

fn distance(left: Point3, right: Point3) -> f64 {
    length(subtract(left, right))
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

    #[test]
    fn lod1_solid_keeps_vertical_walls_and_height() {
        let point3 = |x, y, z| Point3 { x, y, z };
        let ring = |outer| Polygon {
            outer,
            holes: Vec::new(),
            lod: Lod::Lod1,
        };
        let polygons = vec![
            ring(vec![
                point3(0.0, 0.0, 0.0),
                point3(1.0, 0.0, 0.0),
                point3(1.0, 1.0, 0.0),
                point3(0.0, 1.0, 0.0),
            ]),
            ring(vec![
                point3(0.0, 0.0, 10.0),
                point3(1.0, 0.0, 10.0),
                point3(1.0, 1.0, 10.0),
                point3(0.0, 1.0, 10.0),
            ]),
            ring(vec![
                point3(0.0, 0.0, 0.0),
                point3(1.0, 0.0, 0.0),
                point3(1.0, 0.0, 10.0),
                point3(0.0, 0.0, 10.0),
            ]),
            ring(vec![
                point3(1.0, 0.0, 0.0),
                point3(1.0, 1.0, 0.0),
                point3(1.0, 1.0, 10.0),
                point3(1.0, 0.0, 10.0),
            ]),
            ring(vec![
                point3(1.0, 1.0, 0.0),
                point3(0.0, 1.0, 0.0),
                point3(0.0, 1.0, 10.0),
                point3(1.0, 1.0, 10.0),
            ]),
            ring(vec![
                point3(0.0, 1.0, 0.0),
                point3(0.0, 0.0, 0.0),
                point3(0.0, 0.0, 10.0),
                point3(0.0, 1.0, 10.0),
            ]),
        ];

        let geometry = normalize_building_geometry("box", None, &polygons, Lod::Lod1);

        assert_eq!(geometry.triangles.len(), 12);
        let z_values: Vec<_> = geometry
            .triangles
            .iter()
            .flat_map(|triangle| triangle.positions)
            .map(|point| point.z)
            .collect();
        assert!(z_values.iter().copied().fold(f64::INFINITY, f64::min).abs() < f64::EPSILON);
        assert!(
            (z_values.iter().copied().fold(f64::NEG_INFINITY, f64::max) - 10.0).abs()
                < f64::EPSILON
        );
        assert_eq!(
            geometry
                .triangles
                .iter()
                .filter(|triangle| {
                    let [a, b, c] = triangle.positions;
                    (a.z - b.z).abs() > f64::EPSILON || (b.z - c.z).abs() > f64::EPSILON
                })
                .count(),
            8
        );
    }

    #[test]
    fn triangulates_concave_vertical_ring() {
        let point3 = |y, z| Point3 { x: 0.0, y, z };
        let polygon = Polygon {
            outer: vec![
                point3(0.0, 0.0),
                point3(3.0, 0.0),
                point3(3.0, 3.0),
                point3(1.5, 1.0),
                point3(0.0, 3.0),
            ],
            holes: Vec::new(),
            lod: Lod::Lod1,
        };

        assert_eq!(
            normalize_building_geometry("concave", None, &[polygon], Lod::Lod1)
                .triangles
                .len(),
            3
        );
    }
}
