//! Command-line entry point for the `CityGML` to Unity dataset converter.

#[allow(dead_code)]
mod metadata;

use citymodel_citygml::{
    AttributeValue, AxisOrder, BuildingAttribute, Diagnostic, InputLimits, ParserEvent,
    discover_input_files, parse_file,
};
use citymodel_coordinate::Point3;
use citymodel_geometry::{Lod, Polygon, normalize_building_geometry};
use citymodel_gltf::{TileGlbInput, write_tile_glb};
use citymodel_spatialite::{BuildingRow, create_database, insert_building, verify_integrity};
use citymodel_tiling::{DEFAULT_TILE_SIZE_METERS, TileId, tile_for_point};
use rusqlite::params;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

const WORKING_EPSG: u32 = 3857;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Strict,
    Tolerant,
    Inspect,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Command {
    input: PathBuf,
    output: Option<PathBuf>,
    mode: Mode,
    max_lod: u8,
}
#[derive(Clone, Debug)]
struct RawBuilding {
    id: String,
    rings: Vec<RawRing>,
    attributes: Vec<BuildingAttribute>,
    source_file_id: i64,
}
#[derive(Clone, Debug)]
struct RawRing {
    values: Vec<f64>,
    dimension: usize,
    axis_order: AxisOrder,
    lod: u8,
}
#[derive(Clone, Debug)]
struct PreparedBuilding {
    id: String,
    source_file_id: i64,
    triangles: Vec<citymodel_geometry::Triangle>,
    centroid: Point3,
    attributes: Vec<BuildingAttribute>,
    lod_used: u8,
}
#[derive(Clone, Debug)]
struct TileOutput {
    id: String,
    building_ids: Vec<String>,
    glb_path: String,
    metadata_path: String,
    metadata_sha256: String,
    metadata_byte_length: usize,
    glb_sha256: String,
    glb_byte_length: usize,
    bounds: [f64; 4],
    content_bounds: [f64; 6],
    origin: Point3,
    triangle_count: usize,
}

#[derive(Clone, Debug)]
struct BuildingAssignment {
    building_id: String,
    tile_id: String,
    source_file_id: i64,
    feature_id: u16,
    centroid: Point3,
    attributes: Vec<BuildingAttribute>,
    lod_used: u8,
}

#[derive(Clone, Debug)]
struct ConversionIssue {
    source_file_id: i64,
    building_id: Option<String>,
    diagnostic: Diagnostic,
}

fn main() -> ExitCode {
    match parse_command(env::args().skip(1)) {
        Ok(command) => run(command),
        Err(message) => {
            eprintln!(
                "{message}\nusage: citymodel convert <input> --output <directory> [--max-lod <0|1|2>] [--strict|--tolerant]\n       citymodel inspect <input>"
            );
            ExitCode::from(2)
        }
    }
}

fn parse_command(arguments: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut values = arguments.into_iter();
    let action = values.next().ok_or("missing command")?;
    let input = PathBuf::from(values.next().ok_or("missing input")?);
    let mut output = None;
    let mut max_lod = 1;
    let mut mode = match action.as_str() {
        "inspect" => Mode::Inspect,
        "convert" => Mode::Strict,
        _ => return Err("unknown command".to_owned()),
    };
    while let Some(argument) = values.next() {
        match argument.as_str() {
            "--output" => {
                output = Some(PathBuf::from(
                    values.next().ok_or("missing --output value")?,
                ));
            }
            "--strict" => mode = Mode::Strict,
            "--tolerant" => mode = Mode::Tolerant,
            "--max-lod" if action == "convert" => {
                max_lod = values
                    .next()
                    .ok_or("missing --max-lod value")?
                    .parse::<u8>()
                    .map_err(|_| "--max-lod must be 0, 1, or 2")?;
                if max_lod > 2 {
                    return Err("--max-lod must be 0, 1, or 2".to_owned());
                }
            }
            "--max-lod" => return Err("--max-lod is only supported by convert".to_owned()),
            _ => return Err(format!("unknown option: {argument}")),
        }
    }
    Ok(Command {
        input,
        output,
        mode,
        max_lod,
    })
}

fn run(command: Command) -> ExitCode {
    if !command.input.exists() {
        eprintln!("input does not exist: {}", command.input.display());
        return ExitCode::from(1);
    }
    if command.mode == Mode::Inspect {
        return match inspect(&command.input) {
            Ok(value) => {
                println!("{value}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("inspect failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    let Some(output) = command.output else {
        eprintln!("convert requires --output");
        return ExitCode::from(2);
    };
    match atomic_output_handoff(&output, |temporary| {
        convert(&command.input, temporary, command.mode, command.max_lod)
    }) {
        Ok(()) => {
            println!("conversion output: {}", output.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("conversion failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn inspect(input: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let files = discover_input_files(input).map_err(diagnostic_error)?;
    let mut buildings = 0_usize;
    let mut lod_rings = [0_usize; 3];
    for file in &files {
        let report = parse_file(file.clone(), InputLimits::default());
        buildings += report
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    ParserEvent::StartFeature {
                        is_building_part: false,
                        ..
                    }
                )
            })
            .count();
        for (count, lod) in lod_rings.iter_mut().zip([0_u8, 1, 2]) {
            *count += report
                .events
                .iter()
                .filter(|event| match event {
                    ParserEvent::Coordinates(sequence) => {
                        sequence.is_linear_ring && sequence.lod == Some(lod)
                    }
                    _ => false,
                })
                .count();
        }
    }
    Ok(
        json!({"input":input,"mode":"inspect","schemaVersion":citymodel_citygml::contract_schema_version(),"files":files.len(),"buildings":buildings,"lod0Rings":lod_rings[0],"lod1Rings":lod_rings[1],"lod2Rings":lod_rings[2]}),
    )
}

fn convert(
    input: &Path,
    output: &Path,
    mode: Mode,
    max_lod: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let files = discover_input_files(input).map_err(diagnostic_error)?;
    if files.is_empty() {
        return Err("no CityGML files found".into());
    }
    let dataset_id = dataset_id(input);
    let generation_id = format!("gen-{}", &combined_digest(&files)[..16]);
    let mut buildings = Vec::new();
    let mut source_files = Vec::new();
    let mut issues = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let source_file_id = i64::try_from(index + 1).map_err(|_| "too many input files")?;
        let report = parse_file(file.clone(), InputLimits::default());
        if mode == Mode::Strict && !report.diagnostics.is_empty() {
            return Err(format!(
                "{}: {} parser diagnostic(s)",
                file.path.display(),
                report.diagnostics.len()
            )
            .into());
        }
        if mode == Mode::Tolerant {
            issues.extend(
                report
                    .diagnostics
                    .iter()
                    .cloned()
                    .map(|diagnostic| ConversionIssue {
                        source_file_id,
                        building_id: None,
                        diagnostic,
                    }),
            );
        }
        source_files.push((
            source_file_id,
            file.path.clone(),
            file.sha256.clone(),
            fs::metadata(&file.path)?.len(),
        ));
        buildings.extend(extract_buildings(&report.events, source_file_id));
    }
    let prepared = prepare_buildings(buildings, max_lod, mode, &mut issues)?;
    if prepared.is_empty() {
        return Err(format!("no building geometry was found at or below LOD{max_lod}").into());
    }
    let (tile_outputs, assignments) = write_tiles(output, &generation_id, prepared)?;
    let origin = tile_outputs
        .iter()
        .map(|tile| tile.origin)
        .min_by(|left, right| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)))
        .ok_or("no tiles")?;
    let database_path = output.join("citymodel.sqlite");
    write_database(
        &database_path,
        &dataset_id,
        &generation_id,
        &source_files,
        &tile_outputs,
        &assignments,
        &issues,
        origin,
    )?;
    let database_sha256 = sha256_file(&database_path)?;
    write_manifest(
        output,
        &dataset_id,
        &generation_id,
        &source_files,
        &tile_outputs,
        origin,
        &database_sha256,
        max_lod,
    )?;
    fs::write(
        output.join("conversion.report.json"),
        serde_json::to_vec_pretty(
            &json!({"datasetId":dataset_id,"generationId":generation_id,"sourceFiles":source_files.len(),"buildings":assignments.len(),"tiles":tile_outputs.len(),"mode":format!("{mode:?}"),"maxLod":max_lod}),
        )?,
    )?;
    Ok(())
}

fn extract_buildings(events: &[ParserEvent], source_file_id: i64) -> Vec<RawBuilding> {
    #[derive(Clone)]
    struct Active {
        id: String,
        is_part: bool,
        rings: Vec<RawRing>,
        attributes: Vec<BuildingAttribute>,
    }
    let mut active = Vec::<Active>::new();
    let mut output = Vec::new();
    for event in events {
        match event {
            ParserEvent::StartFeature {
                gml_id,
                is_building_part,
            } => active.push(Active {
                id: gml_id.clone(),
                is_part: *is_building_part,
                rings: Vec::new(),
                attributes: Vec::new(),
            }),
            ParserEvent::Coordinates(sequence)
                if sequence.is_linear_ring && matches!(sequence.lod, Some(0..=2)) =>
            {
                if let Some(building) = active.iter_mut().rev().find(|item| !item.is_part) {
                    building.rings.push(RawRing {
                        values: sequence.values.clone(),
                        dimension: usize::from(sequence.dimension.unwrap_or(3)),
                        axis_order: sequence.axis_order,
                        lod: sequence.lod.expect("matched LOD range"),
                    });
                }
            }
            ParserEvent::BuildingAttribute(attribute) => {
                if let Some(building) = active.iter_mut().rev().find(|item| !item.is_part) {
                    building.attributes.push(attribute.clone());
                }
            }
            ParserEvent::EndFeature => {
                if let Some(building) = active.pop() {
                    if !building.is_part {
                        output.push(RawBuilding {
                            id: building.id,
                            rings: building.rings,
                            attributes: building.attributes,
                            source_file_id,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    output
}

#[allow(clippy::cast_precision_loss)]
fn prepare_buildings(
    raw: Vec<RawBuilding>,
    max_lod: u8,
    mode: Mode,
    issues: &mut Vec<ConversionIssue>,
) -> Result<Vec<PreparedBuilding>, Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    let mut unique = BTreeSet::new();
    for building in raw {
        if !unique.insert(building.id.clone()) {
            return Err(format!("duplicate BuildingID: {}", building.id).into());
        }
        let selected_lod = (0..=max_lod)
            .rev()
            .find(|lod| building.rings.iter().any(|ring| ring.lod == *lod));
        let Some(selected_lod) = selected_lod else {
            let message = format!(
                "building {} has no LinearRing geometry at or below requested LOD{}",
                building.id, max_lod
            );
            if mode == Mode::Strict {
                return Err(message.into());
            }
            issues.push(ConversionIssue {
                source_file_id: building.source_file_id,
                building_id: Some(building.id),
                diagnostic: Diagnostic {
                    kind: citymodel_citygml::DiagnosticKind::UnsupportedElement,
                    message,
                },
            });
            continue;
        };
        let polygons: Vec<_> = building
            .rings
            .into_iter()
            .filter(|ring| ring.lod == selected_lod)
            .filter_map(|ring| ring_to_polygon(&ring))
            .collect();
        let lod = lod_from_u8(selected_lod).expect("selected LOD is validated");
        let geometry = normalize_building_geometry(&building.id, None, &polygons, lod);
        if geometry.triangles.is_empty() {
            continue;
        }
        let points = geometry
            .triangles
            .iter()
            .flat_map(|triangle| triangle.positions)
            .collect::<Vec<_>>();
        let count = points.len() as f64;
        let centroid = Point3 {
            x: points.iter().map(|point| point.x).sum::<f64>() / count,
            y: points.iter().map(|point| point.y).sum::<f64>() / count,
            z: points.iter().map(|point| point.z).sum::<f64>() / count,
        };
        output.push(PreparedBuilding {
            id: building.id,
            source_file_id: building.source_file_id,
            triangles: geometry.triangles,
            centroid,
            attributes: building.attributes,
            lod_used: selected_lod,
        });
    }
    Ok(output)
}

fn ring_to_polygon(ring: &RawRing) -> Option<Polygon> {
    if ring.dimension < 2 || ring.values.len() < ring.dimension * 3 {
        return None;
    }
    let outer = ring
        .values
        .chunks(ring.dimension)
        .filter_map(|coordinate| {
            let (north, east) = match ring.axis_order {
                AxisOrder::EastNorthUp => (coordinate.get(1)?, coordinate.first()?),
                AxisOrder::NorthEastUp | AxisOrder::Unknown => {
                    (coordinate.first()?, coordinate.get(1)?)
                }
            };
            Some(web_mercator(
                *east,
                *north,
                coordinate.get(2).copied().unwrap_or(0.0),
            ))
        })
        .collect::<Vec<_>>();
    Some(Polygon {
        outer,
        holes: Vec::new(),
        lod: lod_from_u8(ring.lod)?,
    })
}

fn lod_from_u8(lod: u8) -> Option<Lod> {
    match lod {
        0 => Some(Lod::Lod0),
        1 => Some(Lod::Lod1),
        2 => Some(Lod::Lod2),
        _ => None,
    }
}

#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
fn write_tiles(
    output: &Path,
    generation_id: &str,
    buildings: Vec<PreparedBuilding>,
) -> Result<(Vec<TileOutput>, Vec<BuildingAssignment>), Box<dyn std::error::Error>> {
    let mut grouped = BTreeMap::<TileId, Vec<PreparedBuilding>>::new();
    for building in buildings {
        grouped
            .entry(tile_for_point(
                building.centroid.x,
                building.centroid.y,
                DEFAULT_TILE_SIZE_METERS,
            ))
            .or_default()
            .push(building);
    }
    let mut outputs = Vec::new();
    let mut assignments = Vec::new();
    for (grid, mut buildings) in grouped {
        buildings.sort_by(|left, right| left.id.cmp(&right.id));
        let id = format!("t_{}_{}_{}", grid.level, grid.x, grid.y);
        let origin = Point3 {
            x: grid.x as f64 * DEFAULT_TILE_SIZE_METERS,
            y: grid.y as f64 * DEFAULT_TILE_SIZE_METERS,
            z: 0.0,
        };
        let building_ids: Vec<_> = buildings
            .iter()
            .map(|building| building.id.clone())
            .collect();
        let feature_ids = building_ids
            .iter()
            .enumerate()
            .map(|(index, id)| Ok((id.clone(), u16::try_from(index)?)))
            .collect::<Result<BTreeMap<_, _>, std::num::TryFromIntError>>()?;
        let mut triangles = Vec::new();
        let mut content_bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for building in &buildings {
            let feature = *feature_ids.get(&building.id).ok_or("missing feature id")?;
            assignments.push(BuildingAssignment {
                building_id: building.id.clone(),
                tile_id: id.clone(),
                source_file_id: building.source_file_id,
                feature_id: feature,
                centroid: building.centroid,
                attributes: building.attributes.clone(),
                lod_used: building.lod_used,
            });
            for triangle in &building.triangles {
                let mut local = triangle.clone();
                for point in &mut local.positions {
                    content_bounds[0] = content_bounds[0].min(point.x);
                    content_bounds[1] = content_bounds[1].min(point.y);
                    content_bounds[2] = content_bounds[2].min(point.z);
                    content_bounds[3] = content_bounds[3].max(point.x);
                    content_bounds[4] = content_bounds[4].max(point.y);
                    content_bounds[5] = content_bounds[5].max(point.z);
                    *point = Point3 {
                        x: point.x - origin.x,
                        y: point.z,
                        z: origin.y - point.y,
                    };
                }
                triangles.push(local);
            }
        }
        let triangle_count = triangles.len();
        let asset = write_tile_glb(&TileGlbInput {
            tile_id: id.clone(),
            generation_id: generation_id.to_owned(),
            triangles,
            feature_ids,
        })
        .map_err(|error| format!("GLB write failed: {error:?}"))?;
        let glb_path = format!("tiles/{id}.glb");
        let metadata_path = format!("tiles/{id}.meta.json");
        let glb_output = output.join(&glb_path);
        if let Some(parent) = glb_output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(glb_output, &asset.bytes)?;
        let bounds = [
            origin.x,
            origin.y,
            origin.x + DEFAULT_TILE_SIZE_METERS,
            origin.y + DEFAULT_TILE_SIZE_METERS,
        ];
        let geographic = inverse_web_mercator(origin);
        let metadata_json = metadata::tile_metadata_json(&metadata::TileMetadataInput {
            generation_id,
            tile_id: &id,
            glb_path: &glb_path,
            glb_sha256: &asset.sha256,
            glb_byte_length: asset.bytes.len() as u64,
            building_ids: &building_ids,
            feature_type: "building",
            tile_bounds: bounds,
            content_bounds,
            projected_origin: [origin.x, origin.y, origin.z],
            geographic_origin: [geographic.0, geographic.1, origin.z],
            working_epsg: WORKING_EPSG,
            vertex_count: triangle_count * 3,
            triangle_count,
        });
        let metadata_output = metadata::write_json_under(output, &metadata_path, &metadata_json)?;
        let metadata_byte_length = fs::metadata(&metadata_output)?.len() as usize;
        let metadata_sha256 = sha256_file(&metadata_output)?;
        outputs.push(TileOutput {
            id,
            building_ids,
            glb_path,
            metadata_path,
            metadata_sha256,
            metadata_byte_length,
            glb_sha256: asset.sha256,
            glb_byte_length: asset.bytes.len(),
            bounds,
            content_bounds,
            origin,
            triangle_count,
        });
    }
    Ok((outputs, assignments))
}

#[allow(clippy::cast_possible_wrap, clippy::too_many_arguments)]
fn write_database(
    path: &Path,
    dataset_id: &str,
    generation_id: &str,
    source_files: &[(i64, PathBuf, String, u64)],
    tiles: &[TileOutput],
    assignments: &[BuildingAssignment],
    issues: &[ConversionIssue],
    origin: Point3,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = create_database(path)?;
    connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
    let placeholder = "0".repeat(64);
    connection.execute("INSERT INTO dataset_metadata (dataset_id, schema_version, generation_id, generated_at, generator_name, generator_version, source_crs_epsg, source_crs_wkt, working_crs_epsg, working_crs_wkt, vertical_crs_epsg, vertical_reference_type, axis_order_json, dataset_origin_latitude, dataset_origin_longitude, dataset_origin_height, dataset_origin_geographic_epsg, dataset_origin_x, dataset_origin_y, dataset_origin_z, manifest_sha256, database_sha256, conversion_config_json, license_json) VALUES (?1, '1.0.0', ?2, '1970-01-01T00:00:00Z', 'citymodel', '0.1.0-dev', 6697, NULL, 3857, NULL, NULL, 'source-defined', '[\"latitude\",\"longitude\",\"height\"]', 0.0, 0.0, 0.0, 4326, ?3, ?4, ?5, ?6, ?6, '{}', '{}')", params![dataset_id, generation_id, origin.x, origin.y, origin.z, placeholder])?;
    for (id, file, sha256, length) in source_files {
        connection.execute("INSERT INTO source_files (source_file_id, dataset_id, relative_path, sha256, byte_length) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, dataset_id, file.file_name().and_then(|name| name.to_str()).unwrap_or("input.gml"), sha256, length])?;
    }
    for tile in tiles {
        connection.execute("INSERT INTO tiles (tile_id, dataset_id, generation_id, glb_relative_path, metadata_relative_path, glb_sha256, glb_byte_length, origin_latitude, origin_longitude, origin_height, origin_geographic_epsg, origin_x, origin_y, origin_z, tile_min_x, tile_min_y, tile_max_x, tile_max_y, content_min_x, content_min_y, content_min_z, content_max_x, content_max_y, content_max_z, projected_to_local_matrix_json, building_count, vertex_count, triangle_count, primitive_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0.0, 0.0, 0.0, 4326, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, '[]', ?21, 0, ?22, 1)", params![tile.id, dataset_id, generation_id, tile.glb_path, tile.metadata_path, tile.glb_sha256, tile.glb_byte_length as i64, tile.origin.x, tile.origin.y, tile.origin.z, tile.bounds[0], tile.bounds[1], tile.bounds[2], tile.bounds[3], tile.content_bounds[0], tile.content_bounds[1], tile.content_bounds[2], tile.content_bounds[3], tile.content_bounds[4], tile.content_bounds[5], tile.building_ids.len() as i64, tile.triangle_count as i64])?;
        connection.execute("INSERT INTO tile_contents (tile_id, feature_type, metadata_relative_path, metadata_sha256, metadata_byte_length, glb_relative_path, glb_sha256, glb_byte_length) VALUES (?1, 'building', ?2, ?3, ?4, ?5, ?6, ?7)", params![tile.id, tile.metadata_path, tile.metadata_sha256, tile.metadata_byte_length as i64, tile.glb_path, tile.glb_sha256, tile.glb_byte_length as i64])?;
    }
    for assignment in assignments {
        let canonical = format!("{dataset_id}::{}", assignment.building_id);
        insert_building(
            &connection,
            &BuildingRow {
                building_id: &assignment.building_id,
                canonical_building_id: &canonical,
                gml_id: Some(&assignment.building_id),
                source_file_id: assignment.source_file_id,
                id_source: "gml",
                id_is_synthetic: false,
            },
        )?;
        let attributes_json = attributes_json(&assignment.attributes);
        connection.execute("UPDATE buildings SET tile_id=?1, local_feature_id=?2, lod_used=?3, centroid_x=?4, centroid_y=?5, attributes_json=?6 WHERE building_id=?7", params![assignment.tile_id, i64::from(assignment.feature_id), i64::from(assignment.lod_used), assignment.centroid.x, assignment.centroid.y, attributes_json, assignment.building_id])?;
        connection.execute("INSERT INTO tile_features (tile_id, local_feature_id, building_id, building_part_id) VALUES (?1, ?2, ?3, NULL)", params![assignment.tile_id, i64::from(assignment.feature_id), assignment.building_id])?;
        connection.execute("INSERT INTO features (feature_id, canonical_feature_id, feature_type, gml_id, id_source, id_is_synthetic, source_file_id) VALUES (?1, ?2, 'building', ?1, 'gml', 0, ?3)", params![assignment.building_id, canonical, assignment.source_file_id])?;
        connection.execute("INSERT INTO feature_tile_mappings (tile_id, feature_type, local_feature_id, feature_id) VALUES (?1, 'building', ?2, ?3)", params![assignment.tile_id, i64::from(assignment.feature_id), assignment.building_id])?;
        insert_attributes(&connection, &assignment.building_id, &assignment.attributes)?;
        insert_feature_attributes(&connection, &assignment.building_id, &assignment.attributes)?;
    }
    for issue in issues {
        connection.execute(
            "INSERT INTO conversion_issues (source_file_id, building_id, gml_id, severity, error_code, message, element_path, repaired, exclusion_reason, occurred_at) VALUES (?1, ?2, ?2, 'warn', ?3, ?4, NULL, 0, NULL, '1970-01-01T00:00:00Z')",
            params![issue.source_file_id, issue.building_id, format!("{:?}", issue.diagnostic.kind), issue.diagnostic.message],
        )?;
    }
    verify_integrity(&connection)?;
    Ok(())
}

fn insert_attributes(
    connection: &rusqlite::Connection,
    building_id: &str,
    attributes: &[BuildingAttribute],
) -> rusqlite::Result<()> {
    let mut ordinals = BTreeMap::<(String, String), i64>::new();
    for attribute in attributes {
        let key = (
            attribute.namespace_uri.clone(),
            attribute.attribute_path.clone(),
        );
        let ordinal = ordinals.entry(key).or_insert(0);
        let (value_type, value_text, value_real) = match &attribute.value {
            AttributeValue::Code(value) => ("code", Some(value.as_str()), None),
            AttributeValue::Real(value) => ("real", None, Some(*value)),
        };
        connection.execute(
            "INSERT INTO building_attributes (building_id, namespace_uri, attribute_path, attribute_key, ordinal, value_type, value_text, value_real, value_integer, value_boolean, value_datetime, uom, code_space, nil_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, ?9, ?10, ?11)",
            params![building_id, attribute.namespace_uri, attribute.attribute_path, attribute.attribute_key, *ordinal, value_type, value_text, value_real, attribute.uom, attribute.code_space, attribute.nil_reason],
        )?;
        *ordinal += 1;
    }
    Ok(())
}

fn insert_feature_attributes(
    connection: &rusqlite::Connection,
    feature_id: &str,
    attributes: &[BuildingAttribute],
) -> rusqlite::Result<()> {
    let mut ordinals = BTreeMap::<(String, String), i64>::new();
    for attribute in attributes {
        let key = (
            attribute.namespace_uri.clone(),
            attribute.attribute_path.clone(),
        );
        let ordinal = ordinals.entry(key).or_insert(0);
        let (value_type, value_text, value_real) = match &attribute.value {
            AttributeValue::Code(value) => ("code", Some(value.as_str()), None),
            AttributeValue::Real(value) => ("real", None, Some(*value)),
        };
        connection.execute(
            "INSERT INTO feature_attributes (feature_id, namespace_uri, attribute_path, attribute_key, ordinal, value_type, value_text, value_real, value_integer, value_boolean, value_datetime, uom, code_space, nil_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, ?9, ?10, ?11)",
            params![feature_id, attribute.namespace_uri, attribute.attribute_path, attribute.attribute_key, *ordinal, value_type, value_text, value_real, attribute.uom, attribute.code_space, attribute.nil_reason],
        )?;
        *ordinal += 1;
    }
    Ok(())
}

fn attributes_json(attributes: &[BuildingAttribute]) -> String {
    let values = attributes
        .iter()
        .map(|attribute| {
            let (value_type, value) = match &attribute.value {
                AttributeValue::Code(value) => ("code", json!(value)),
                AttributeValue::Real(value) => ("real", json!(value)),
            };
            json!({
                "namespaceUri": attribute.namespace_uri,
                "attributePath": attribute.attribute_path,
                "attributeKey": attribute.attribute_key,
                "valueType": value_type,
                "value": value,
                "uom": attribute.uom,
                "codeSpace": attribute.code_space,
                "nilReason": attribute.nil_reason,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&values).expect("attribute JSON serialization cannot fail")
}

#[allow(clippy::too_many_arguments)]
fn write_manifest(
    output: &Path,
    dataset_id: &str,
    generation_id: &str,
    source_files: &[(i64, PathBuf, String, u64)],
    tiles: &[TileOutput],
    origin: Point3,
    database_sha256: &str,
    max_lod: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let geographic = inverse_web_mercator(origin);
    let input_files = source_files.iter().map(|(_, path, sha256, _)| json!({"path":path.file_name().and_then(|name| name.to_str()).unwrap_or("input.gml"),"sha256":sha256})).collect::<Vec<_>>();
    let items = tiles
        .iter()
        .map(|tile| json!({"tileId":tile.id,"metadata":tile.metadata_path,"contents":[{"featureType":"building","metadata":tile.metadata_path,"sha256":tile.metadata_sha256,"byteLength":tile.metadata_byte_length}]}))
        .collect::<Vec<_>>();
    let manifest = json!({"schemaVersion":"1.0.0","datasetId":dataset_id,"generationId":generation_id,"generatedAt":"1970-01-01T00:00:00Z","generator":{"name":"citymodel","version":"0.1.0-dev"},"source":{"format":"CityGML","profile":"PLATEAU","citygmlVersion":"2.0","files":source_files.len(),"inputFiles":input_files,"conversionConfiguration":{"lod":max_lod,"maxLod":max_lod,"lodSelection":"highest-available-at-or-below-max-lod","tileSizeMetres":DEFAULT_TILE_SIZE_METERS,"workingCrs":"EPSG:3857"}},"coordinateReference":{"sourceCrs":{"epsg":6697,"wkt":null,"axisOrder":["latitude","longitude","height"]},"workingCrs":{"epsg":WORKING_EPSG,"wkt":null,"axisOrder":["easting","northing","height"],"unit":"metre"},"verticalReference":{"type":"source-defined","epsg":null,"geoidModel":null}},"datasetOrigin":{"geographic":{"latitude":geographic.0,"longitude":geographic.1,"height":origin.z,"epsg":4326},"projected":{"x":origin.x,"y":origin.y,"z":origin.z,"epsg":WORKING_EPSG}},"tiling":{"scheme":"projected-grid","defaultTileSizeMetres":DEFAULT_TILE_SIZE_METERS,"buildingAssignment":"representative-point","geometryClipping":false},"modelProfile":{"lod":max_lod,"textures":false,"compression":null,"featureIdSemantic":"_FEATURE_ID_0","featureIdComponentType":"UNSIGNED_SHORT"},"database":{"path":"citymodel.sqlite","sha256":database_sha256},"tiles":{"indexType":"inline","items":items}});
    fs::write(
        output.join("dataset.manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn dataset_id(input: &Path) -> String {
    input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("citymodel-dataset")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}
fn combined_digest(files: &[citymodel_citygml::InputFile]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.sha256.as_bytes());
    }
    hex(hasher.finalize())
}
fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(hex(Sha256::digest(fs::read(path)?)))
}
fn hex(digest: impl IntoIterator<Item = u8>) -> String {
    digest.into_iter().fold(String::new(), |mut text, byte| {
        write!(text, "{byte:02x}").expect("writing to String cannot fail");
        text
    })
}
fn diagnostic_error(diagnostic: citymodel_citygml::Diagnostic) -> Box<dyn std::error::Error> {
    diagnostic.message.into()
}
fn web_mercator(longitude: f64, latitude: f64, height: f64) -> Point3 {
    let latitude = latitude.clamp(-85.051_128_78, 85.051_128_78).to_radians();
    Point3 {
        x: longitude.to_radians() * 6_378_137.0,
        y: (std::f64::consts::FRAC_PI_4 + latitude / 2.0).tan().ln() * 6_378_137.0,
        z: height,
    }
}
fn inverse_web_mercator(point: Point3) -> (f64, f64) {
    let longitude = point.x / 6_378_137.0;
    let latitude = 2.0 * (point.y / 6_378_137.0).exp().atan() - std::f64::consts::FRAC_PI_2;
    (latitude.to_degrees(), longitude.to_degrees())
}
fn atomic_output_handoff(
    output: &Path,
    write: impl FnOnce(&Path) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = output.with_extension("tmp-citymodel");
    if temporary.exists() {
        fs::remove_dir_all(&temporary)?;
    }
    fs::create_dir_all(&temporary)?;
    if let Err(error) = write(&temporary) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    if output.exists() {
        fs::remove_dir_all(output)?;
    }
    fs::rename(temporary, output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type StoredAttribute = (
        String,
        String,
        Option<String>,
        Option<f64>,
        Option<String>,
        Option<String>,
    );
    #[test]
    fn parses_inspect_and_tolerant_convert() {
        assert_eq!(
            parse_command(["inspect".into(), "x.gml".into()])
                .unwrap()
                .mode,
            Mode::Inspect
        );
        assert_eq!(
            parse_command([
                "convert".into(),
                "x.gml".into(),
                "--tolerant".into(),
                "--output".into(),
                "out".into()
            ])
            .unwrap()
            .mode,
            Mode::Tolerant
        );
        assert_eq!(
            parse_command([
                "convert".into(),
                "x.gml".into(),
                "--output".into(),
                "out".into(),
                "--max-lod".into(),
                "2".into(),
            ])
            .unwrap()
            .max_lod,
            2
        );
        assert!(
            parse_command([
                "convert".into(),
                "x.gml".into(),
                "--output".into(),
                "out".into(),
                "--max-lod".into(),
                "3".into(),
            ])
            .is_err()
        );
    }
    #[test]
    fn web_mercator_round_trips() {
        let point = web_mercator(139.6, 35.8, 5.0);
        let (latitude, longitude) = inverse_web_mercator(point);
        assert!((latitude - 35.8).abs() < 0.000_001);
        assert!((longitude - 139.6).abs() < 0.000_001);
    }

    #[test]
    fn converts_lod1_fixture_to_a_unity_dataset() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../citymodel-citygml/tests/fixtures/plateau-lod1-small.gml");
        let output = std::env::temp_dir().join(format!("citymodel-cli-e2e-{}", std::process::id()));
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&output).unwrap();
        convert(&fixture, &output, Mode::Strict, 1).unwrap();

        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("dataset.manifest.json")).unwrap())
                .unwrap();
        let metadata = manifest["tiles"]["items"][0]["metadata"].as_str().unwrap();
        let content_index = &manifest["tiles"]["items"][0]["contents"][0];
        assert_eq!(content_index["featureType"], "building");
        assert_eq!(content_index["metadata"], metadata);
        assert_eq!(
            content_index["byteLength"].as_u64().unwrap() as usize,
            fs::metadata(output.join(metadata)).unwrap().len() as usize
        );
        assert_eq!(
            content_index["sha256"],
            sha256_file(&output.join(metadata)).unwrap()
        );
        let tile: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join(metadata)).unwrap()).unwrap();
        assert_eq!(tile["content"]["featureType"], "building");
        assert_eq!(tile["features"]["items"][0]["featureType"], "building");
        assert_eq!(
            tile["features"]["items"][0]["featureId"],
            "sample-building-1"
        );
        let glb = fs::read(output.join(tile["content"]["glb"].as_str().unwrap())).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        assert!(output.join("citymodel.sqlite").is_file());
        let database = rusqlite::Connection::open(output.join("citymodel.sqlite")).unwrap();
        let attributes: Vec<StoredAttribute> = database
            .prepare("SELECT attribute_key, value_type, value_text, value_real, uom, code_space FROM building_attributes ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            attributes,
            vec![
                (
                    "usage".to_owned(),
                    "code".to_owned(),
                    Some("residential".to_owned()),
                    None,
                    None,
                    Some("https://example.test/usage".to_owned())
                ),
                (
                    "measuredHeight".to_owned(),
                    "real".to_owned(),
                    None,
                    Some(12.5),
                    Some("m".to_owned()),
                    None
                ),
            ]
        );
        let attributes_json: String = database
            .query_row(
                "SELECT attributes_json FROM buildings WHERE building_id = 'sample-building-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&attributes_json).unwrap()[0]["namespaceUri"],
            "http://www.opengis.net/citygml/building/2.0"
        );
        let common_feature: (String, String) = database.query_row(
            "SELECT feature_id, feature_type FROM features WHERE feature_id = 'sample-building-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(
            common_feature,
            ("sample-building-1".to_owned(), "building".to_owned())
        );
        let common_attribute_count: i64 = database
            .query_row(
                "SELECT COUNT(*) FROM feature_attributes WHERE feature_id = 'sample-building-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(common_attribute_count, 2);
        let common_mapping: String = database
            .query_row(
                "SELECT feature_id FROM feature_tile_mappings WHERE feature_type = 'building' AND local_feature_id = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(common_mapping, "sample-building-1");
        let common_content: (String, String, String) = database
            .query_row(
                "SELECT feature_type, metadata_relative_path, glb_relative_path FROM tile_contents",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(common_content.0, "building");
        assert_eq!(common_content.1, metadata);
        assert_eq!(common_content.2, tile["content"]["glb"].as_str().unwrap());
        let user_version: i64 = database
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, 2);
        drop(database);
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn tolerant_conversion_records_invalid_attribute_diagnostics() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../citymodel-citygml/tests/fixtures/plateau-lod1-small.gml");
        let input = std::env::temp_dir().join(format!(
            "citymodel-cli-invalid-attribute-{}.gml",
            std::process::id()
        ));
        fs::write(
            &input,
            fs::read_to_string(&fixture)
                .unwrap()
                .replace(">12.5</b:measuredHeight>", ">high</b:measuredHeight>"),
        )
        .unwrap();
        let strict_output = std::env::temp_dir().join(format!(
            "citymodel-cli-invalid-attribute-strict-{}",
            std::process::id()
        ));
        let tolerant_output = std::env::temp_dir().join(format!(
            "citymodel-cli-invalid-attribute-tolerant-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&strict_output);
        let _ = fs::remove_dir_all(&tolerant_output);
        assert!(convert(&input, &strict_output, Mode::Strict, 1).is_err());
        fs::create_dir_all(&tolerant_output).unwrap();
        convert(&input, &tolerant_output, Mode::Tolerant, 1).unwrap();
        let database =
            rusqlite::Connection::open(tolerant_output.join("citymodel.sqlite")).unwrap();
        let issue: (String, String) = database
            .query_row(
                "SELECT error_code, message FROM conversion_issues",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(issue.0, "InvalidAttribute");
        assert!(issue.1.contains("measuredHeight"));
        drop(database);
        fs::remove_file(input).unwrap();
        fs::remove_dir_all(tolerant_output).unwrap();
    }

    #[test]
    fn max_lod_selects_highest_available_lod_per_building() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/plateau-multi-lod-small.gml");
        let output =
            std::env::temp_dir().join(format!("citymodel-cli-multi-lod-{}", std::process::id()));
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&output).unwrap();

        convert(&fixture, &output, Mode::Strict, 2).unwrap();
        let database = rusqlite::Connection::open(output.join("citymodel.sqlite")).unwrap();
        let selected: Vec<(String, i64)> = database
            .prepare("SELECT building_id, lod_used FROM buildings ORDER BY building_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            selected,
            vec![
                ("building-all".to_owned(), 2),
                ("building-lod0".to_owned(), 0),
                ("building-lod1".to_owned(), 1),
                ("building-lod2-only".to_owned(), 2),
            ]
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("dataset.manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["source"]["conversionConfiguration"]["maxLod"], 2);
        assert_eq!(manifest["modelProfile"]["lod"], 2);
        drop(database);

        fs::remove_dir_all(&output).unwrap();
    }

    #[test]
    fn tolerant_max_lod_records_buildings_without_a_permitted_lod() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/plateau-multi-lod-small.gml");
        let output = std::env::temp_dir().join(format!(
            "citymodel-cli-multi-lod-tolerant-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&output).unwrap();

        assert!(convert(&fixture, &output, Mode::Strict, 1).is_err());
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&output).unwrap();
        convert(&fixture, &output, Mode::Tolerant, 1).unwrap();
        let database = rusqlite::Connection::open(output.join("citymodel.sqlite")).unwrap();
        let selected: Vec<(String, i64)> = database
            .prepare("SELECT building_id, lod_used FROM buildings ORDER BY building_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            selected,
            vec![
                ("building-all".to_owned(), 0),
                ("building-lod0".to_owned(), 0),
                ("building-lod1".to_owned(), 1),
            ]
        );
        let issue: (String, String) = database
            .query_row(
                "SELECT building_id, error_code FROM conversion_issues",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            issue,
            (
                "building-lod2-only".to_owned(),
                "UnsupportedElement".to_owned()
            )
        );
        drop(database);
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn inspect_reports_linear_ring_counts_for_each_supported_lod() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/plateau-multi-lod-small.gml");
        let report = inspect(&fixture).unwrap();
        assert_eq!(report["lod0Rings"], 2);
        assert_eq!(report["lod1Rings"], 1);
        assert_eq!(report["lod2Rings"], 2);
    }
}
