//! Coordinate reference metadata and lossless local-coordinate transforms.

pub const MODULE_NAME: &str = "citymodel-coordinate";
#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AxisOrder {
    EastNorthUp,
    NorthEastUp,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeightReference {
    Ellipsoidal,
    Orthometric,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrsMetadata {
    pub source_crs: String,
    pub working_crs: String,
    pub axis_order: AxisOrder,
    pub height_reference: HeightReference,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Origins {
    pub dataset: Point3,
    pub tile: Point3,
    pub scene: Point3,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalCoordinateTransform {
    pub origins: Origins,
}

impl LocalCoordinateTransform {
    /// `X = East`, `Y = Up`, and `Z = -North` relative to the tile origin.
    #[must_use]
    pub fn working_to_glb(&self, working: Point3) -> Point3 {
        Point3 {
            x: working.x - self.origins.tile.x,
            y: working.z - self.origins.tile.z,
            z: self.origins.tile.y - working.y,
        }
    }
    #[must_use]
    pub fn glb_to_working(&self, glb: Point3) -> Point3 {
        Point3 {
            x: glb.x + self.origins.tile.x,
            y: self.origins.tile.y - glb.z,
            z: glb.y + self.origins.tile.z,
        }
    }
    /// Changes Scene Origin by changing placement only; GLB vertices remain unchanged.
    #[must_use]
    pub fn glb_to_unity(&self, glb: Point3) -> Point3 {
        Point3 {
            x: glb.x + self.origins.tile.x - self.origins.scene.x,
            y: glb.y + self.origins.tile.z - self.origins.scene.z,
            z: glb.z + self.origins.scene.y - self.origins.tile.y,
        }
    }
    #[must_use]
    pub fn unity_to_glb(&self, unity: Point3) -> Point3 {
        Point3 {
            x: unity.x - self.origins.tile.x + self.origins.scene.x,
            y: unity.y - self.origins.tile.z + self.origins.scene.z,
            z: unity.z - self.origins.scene.y + self.origins.tile.y,
        }
    }
}

/// Selects the closest JGD2011 Japanese plane-rectangular CRS (EPSG:6669–6687).
#[must_use]
pub fn select_japan_plane_crs(longitude_degrees: f64) -> Option<String> {
    if !(122.0..=156.0).contains(&longitude_degrees) {
        return None;
    }
    let meridians = [
        129.5,
        131.0,
        132.166_666_7,
        133.5,
        134.333_333_3,
        136.0,
        137.166_666_7,
        138.5,
        139.833_333_3,
        140.833_333_3,
        140.25,
        142.25,
        144.25,
        142.0,
        127.5,
        124.0,
        131.0,
        136.0,
        154.0,
    ];
    let zone = meridians
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            (longitude_degrees - **left)
                .abs()
                .total_cmp(&(longitude_degrees - **right).abs())
        })?
        .0;
    Some(format!("EPSG:{}", 6669 + zone))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glb_and_unity_round_trip() {
        let transform = LocalCoordinateTransform {
            origins: Origins {
                dataset: Point3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                tile: Point3 {
                    x: 100.0,
                    y: 200.0,
                    z: 5.0,
                },
                scene: Point3 {
                    x: 50.0,
                    y: 150.0,
                    z: 2.0,
                },
            },
        };
        let working = Point3 {
            x: 101.0,
            y: 203.0,
            z: 9.0,
        };
        let glb = transform.working_to_glb(working);
        assert_eq!(transform.glb_to_working(glb), working);
        assert_eq!(transform.unity_to_glb(transform.glb_to_unity(glb)), glb);
    }
}
