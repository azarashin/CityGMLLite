//! Command-line entry point for the `CityGML` to Unity dataset converter.

#[allow(dead_code)]
mod metadata;

use citymodel_citygml::{
    AttributeValue, AxisOrder, BuildingAttribute, Diagnostic, DiagnosticKind, FeatureType,
    InputLimits, ParserEvent, TerrainTexture as ParsedTerrainTexture, discover_input_files,
    discover_input_paths, hash_input_file, parse_file,
};
use citymodel_coordinate::Point3;
use citymodel_geometry::{Lod, Polygon, normalize_building_geometry};
use citymodel_gltf::{
    TerrainGlbInput, TerrainTexture, TerrainTriangle, TileGlbInput, write_terrain_glb,
    write_tile_glb,
};
use citymodel_spatialite::{BuildingRow, create_database, insert_building, verify_integrity};
use citymodel_tiling::{DEFAULT_TILE_SIZE_METERS, TileId, tile_for_point};
use quick_xml::Reader;
use quick_xml::events::Event;
use rusqlite::params;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::Write as _,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

const WORKING_EPSG: u32 = 3857;
const MAX_TERRAIN_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TERRAIN_TEXTURE_DECODED_RGBA_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ENVELOPE_CORNER_TEXT_BYTES: usize = 1024;

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
struct RawTerrain {
    id: String,
    source_file_id: i64,
    rings: Vec<RawTerrainRing>,
}
#[derive(Clone, Debug)]
struct RawTerrainRing {
    surface_id: String,
    values: Vec<f64>,
    dimension: usize,
    axis_order: AxisOrder,
}
#[derive(Clone, Copy, Debug)]
struct GeographicEnvelope {
    south: f64,
    west: f64,
    north: f64,
    east: f64,
}
#[derive(Clone, Debug)]
struct TerrainTextureDeclaration {
    source_file_id: i64,
    texture: ParsedTerrainTexture,
}
#[derive(Clone, Debug)]
struct TerrainMapTexture {
    image_uri: String,
    envelope: GeographicEnvelope,
}
#[derive(Clone, Debug)]
struct TerrainTileOutput {
    id: String,
    feature_assignments: Vec<(String, i64, u16)>,
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

#[derive(Clone, Debug)]
struct InputFileBreakdown {
    relative_path: String,
    byte_length: u64,
    parsing_elapsed_ms: u128,
    raw_buildings: usize,
    raw_terrain_features: usize,
    terrain_texture_declarations: usize,
    diagnostics: usize,
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
            let report_path = output.join("conversion.report.json");
            let total_elapsed_ms = fs::read(&report_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|report| report["totalElapsedMs"].as_u64());
            match total_elapsed_ms {
                Some(milliseconds) => println!(
                    "conversion report: {} (total: {milliseconds} ms)",
                    report_path.display()
                ),
                None => println!("conversion report: {}", report_path.display()),
            }
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

#[allow(clippy::too_many_lines)]
fn convert(
    input: &Path,
    output: &Path,
    mode: Mode,
    max_lod: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let total_started = Instant::now();
    let stage_started = Instant::now();
    status_stage_started("input discovery and hashing");
    let candidate_paths = discover_input_paths(input).map_err(diagnostic_error)?;
    if candidate_paths.is_empty() {
        return Err("no CityGML files found".into());
    }
    let input_root = input_root(input)?;
    let input_file_sizes = candidate_paths
        .iter()
        .map(|path| Ok((path.clone(), fs::metadata(path)?.len())))
        .collect::<Result<BTreeMap<_, _>, std::io::Error>>()?;
    let total_input_bytes = input_file_sizes.values().sum::<u64>();
    eprintln!(
        "[citymodel] input candidate discovery: finished ({} file(s), {} bytes)",
        candidate_paths.len(),
        total_input_bytes
    );
    for (index, path) in candidate_paths.iter().enumerate() {
        let bytes = input_file_sizes
            .get(path)
            .copied()
            .ok_or("input file size missing")?;
        eprintln!(
            "[citymodel] input [{}/{}] {} ({} bytes)",
            index + 1,
            candidate_paths.len(),
            input_relative_path(path, input_root).display(),
            bytes
        );
    }
    let mut files = Vec::with_capacity(candidate_paths.len());
    for (index, path) in candidate_paths.iter().enumerate() {
        let bytes = input_file_sizes
            .get(path)
            .copied()
            .ok_or("input file size missing")?;
        let relative_path = input_relative_path(path, input_root);
        let hash_started = Instant::now();
        eprintln!(
            "[citymodel] hashing [{}/{}] started: {} ({} bytes)",
            index + 1,
            candidate_paths.len(),
            relative_path.display(),
            bytes
        );
        let file = hash_input_file(path).map_err(diagnostic_error)?;
        eprintln!(
            "[citymodel] hashing [{}/{}] finished: {} ({} bytes; {} ms)",
            index + 1,
            candidate_paths.len(),
            relative_path.display(),
            bytes,
            elapsed_ms(hash_started)
        );
        files.push(file);
    }
    let dataset_id = dataset_id(input);
    let generation_id = format!("gen-{}", &combined_digest(&files)[..16]);
    let discovery_elapsed_ms = elapsed_ms(stage_started);
    status_stage_finished("input discovery and hashing", discovery_elapsed_ms);
    let mut buildings = Vec::new();
    let mut terrain = Vec::new();
    let mut terrain_textures = Vec::new();
    let mut terrain_envelopes = BTreeMap::new();
    let mut source_files = Vec::new();
    let mut issues = Vec::new();
    let mut input_file_breakdown = Vec::new();
    let stage_started = Instant::now();
    status_stage_started("CityGML parsing and extraction");
    for (index, file) in files.iter().enumerate() {
        let source_file_id = i64::try_from(index + 1).map_err(|_| "too many input files")?;
        let byte_length = input_file_sizes
            .get(&file.path)
            .copied()
            .ok_or("input file size missing")?;
        let relative_path = input_relative_path(&file.path, input_root)
            .to_string_lossy()
            .into_owned();
        let file_started = Instant::now();
        eprintln!(
            "[citymodel] parsing [{}/{}] started: {} ({} bytes)",
            index + 1,
            files.len(),
            relative_path,
            byte_length
        );
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
            byte_length,
        ));
        let file_buildings = extract_buildings(&report.events, source_file_id);
        let (file_terrain, file_textures) = extract_terrain(&report.events, source_file_id);
        let file_envelope = geographic_envelope_from_file(&file.path).ok().flatten();
        let file_elapsed_ms = elapsed_ms(file_started);
        eprintln!(
            "[citymodel] parsing [{}/{}] finished: {} ({} ms; buildings: {}, terrain: {}, textures: {}, diagnostics: {})",
            index + 1,
            files.len(),
            relative_path,
            file_elapsed_ms,
            file_buildings.len(),
            file_terrain.len(),
            file_textures.len(),
            report.diagnostics.len()
        );
        input_file_breakdown.push(InputFileBreakdown {
            relative_path,
            byte_length,
            parsing_elapsed_ms: file_elapsed_ms,
            raw_buildings: file_buildings.len(),
            raw_terrain_features: file_terrain.len(),
            terrain_texture_declarations: file_textures.len(),
            diagnostics: report.diagnostics.len(),
        });
        buildings.extend(file_buildings);
        terrain.extend(file_terrain);
        terrain_textures.extend(file_textures.into_iter().map(|texture| {
            TerrainTextureDeclaration {
                source_file_id,
                texture,
            }
        }));
        if let Some(envelope) = file_envelope {
            terrain_envelopes.insert(source_file_id, envelope);
        }
    }
    let parsing_elapsed_ms = elapsed_ms(stage_started);
    status_stage_finished("CityGML parsing and extraction", parsing_elapsed_ms);
    let raw_building_count = buildings.len();
    let raw_terrain_count = terrain.len();
    let terrain_texture_declaration_count = terrain_textures.len();

    let stage_started = Instant::now();
    status_stage_started("building geometry preparation");
    let prepared = prepare_buildings(buildings, max_lod, mode, &mut issues)
        .map_err(|error| stage_error("building geometry preparation", error))?;
    let building_triangle_count = prepared
        .iter()
        .map(|building| building.triangles.len())
        .sum::<usize>();
    let preparation_elapsed_ms = elapsed_ms(stage_started);
    status_stage_finished("building geometry preparation", preparation_elapsed_ms);

    let stage_started = Instant::now();
    status_stage_started("building GLB tile generation");
    let (tile_outputs, assignments) = write_tiles(output, &generation_id, prepared)
        .map_err(|error| stage_error("building GLB tile write", error))?;
    let building_glb_elapsed_ms = elapsed_ms(stage_started);
    status_stage_finished("building GLB tile generation", building_glb_elapsed_ms);
    let stage_started = Instant::now();
    status_stage_started("terrain GLB tile generation");
    let terrain_outputs = write_terrain_tiles(
        output,
        &generation_id,
        terrain,
        terrain_textures,
        input_root,
        &source_files,
        &terrain_envelopes,
        &mut issues,
    )
    .map_err(|error| stage_error("terrain GLB tile write", error))?;
    let terrain_glb_elapsed_ms = elapsed_ms(stage_started);
    status_stage_finished("terrain GLB tile generation", terrain_glb_elapsed_ms);
    if tile_outputs.is_empty() && terrain_outputs.is_empty() {
        return Err(format!(
            "no building or textured terrain geometry was found at or below LOD{max_lod}"
        )
        .into());
    }
    let origin = tile_outputs
        .iter()
        .map(|tile| tile.origin)
        .min_by(|left, right| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)))
        .unwrap_or(Point3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
    let database_path = output.join("citymodel.sqlite");
    let stage_started = Instant::now();
    status_stage_started("SQLite write");
    write_database(
        &database_path,
        &dataset_id,
        &generation_id,
        &source_files,
        &tile_outputs,
        &terrain_outputs,
        &assignments,
        &issues,
        origin,
    )
    .map_err(|error| stage_error("SQLite write", error))?;
    let sqlite_write_elapsed_ms = elapsed_ms(stage_started);
    status_stage_finished("SQLite write", sqlite_write_elapsed_ms);
    let database_byte_length = fs::metadata(&database_path)?.len();
    let database_row_counts = database_row_counts(&database_path)?;

    let stage_started = Instant::now();
    status_stage_started("database hash");
    let database_sha256 =
        sha256_file(&database_path).map_err(|error| stage_error("database hash", error))?;
    let database_hash_elapsed_ms = elapsed_ms(stage_started);
    status_stage_finished("database hash", database_hash_elapsed_ms);

    let stage_started = Instant::now();
    status_stage_started("manifest write");
    write_manifest(
        output,
        &dataset_id,
        &generation_id,
        &source_files,
        &tile_outputs,
        &terrain_outputs,
        origin,
        &database_sha256,
        max_lod,
    )
    .map_err(|error| stage_error("manifest write", error))?;
    let manifest_write_elapsed_ms = elapsed_ms(stage_started);
    status_stage_finished("manifest write", manifest_write_elapsed_ms);

    let building_glb_byte_length = tile_outputs
        .iter()
        .map(|tile| tile.glb_byte_length)
        .sum::<usize>();
    let terrain_glb_byte_length = terrain_outputs
        .iter()
        .map(|tile| tile.glb_byte_length)
        .sum::<usize>();
    let terrain_triangle_count = terrain_outputs
        .iter()
        .map(|tile| tile.triangle_count)
        .sum::<usize>();
    let stage_elapsed_ms = [
        ("inputDiscoveryAndHashing", discovery_elapsed_ms),
        ("parsingAndExtraction", parsing_elapsed_ms),
        ("buildingGeometryPreparation", preparation_elapsed_ms),
        ("buildingGlbTiles", building_glb_elapsed_ms),
        ("terrainGlbTiles", terrain_glb_elapsed_ms),
        ("sqliteWrite", sqlite_write_elapsed_ms),
        ("databaseHash", database_hash_elapsed_ms),
        ("manifestWrite", manifest_write_elapsed_ms),
    ];
    let report_started = Instant::now();
    let report_path = output.join("conversion.report.json");
    let total_elapsed_ms = elapsed_ms(total_started);
    let terrain_texture_fallbacks = issues
        .iter()
        .filter(|issue| {
            issue
                .diagnostic
                .message
                .starts_with("terrain map texture fallback")
        })
        .map(|issue| {
            json!({
                "sourceFileId": issue.source_file_id,
                "featureId": issue.building_id,
                "message": issue.diagnostic.message,
            })
        })
        .collect::<Vec<_>>();
    let summary = conversion_summary(
        total_elapsed_ms,
        &stage_elapsed_ms,
        &input_file_breakdown,
        raw_building_count,
        raw_terrain_count,
        assignments.len(),
        tile_outputs.len(),
        terrain_outputs.len(),
        building_triangle_count,
        terrain_triangle_count,
        building_glb_byte_length,
        terrain_glb_byte_length,
        database_byte_length,
        issues.len(),
    );
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&json!({
            "summary": summary,
            "datasetId":dataset_id,
            "generationId":generation_id,
            "sourceFiles":source_files.len(),
            "buildings":assignments.len(),
            "terrainTiles":terrain_outputs.len(),
            "terrainTextureFallbacks": terrain_texture_fallbacks,
            "tiles":tile_outputs.len(),
            "mode":format!("{mode:?}"),
            "maxLod":max_lod,
            "totalElapsedMs": total_elapsed_ms,
            "inputFiles": input_file_breakdown.iter().map(|file| json!({
                "relativePath": file.relative_path,
                "byteLength": file.byte_length,
                "parsingElapsedMs": file.parsing_elapsed_ms,
                "rawBuildings": file.raw_buildings,
                "rawTerrainFeatures": file.raw_terrain_features,
                "terrainTextureDeclarations": file.terrain_texture_declarations,
                "diagnostics": file.diagnostics
            })).collect::<Vec<_>>(),
            "stages": {
                "inputDiscoveryAndHashing": {"elapsedMs": discovery_elapsed_ms, "inputFiles": files.len()},
                "parsingAndExtraction": {"elapsedMs": parsing_elapsed_ms, "inputBytes": source_files.iter().map(|(_, _, _, length)| length).sum::<u64>(), "rawBuildings": raw_building_count, "rawTerrainFeatures": raw_terrain_count, "terrainTextureDeclarations": terrain_texture_declaration_count, "diagnostics": issues.len()},
                "buildingGeometryPreparation": {"elapsedMs": preparation_elapsed_ms, "preparedBuildings": assignments.len(), "triangles": building_triangle_count},
                "buildingGlbTiles": {"elapsedMs": building_glb_elapsed_ms, "tiles": tile_outputs.len(), "triangles": tile_outputs.iter().map(|tile| tile.triangle_count).sum::<usize>(), "glbBytes": building_glb_byte_length},
                "terrainGlbTiles": {"elapsedMs": terrain_glb_elapsed_ms, "tiles": terrain_outputs.len(), "triangles": terrain_triangle_count, "glbBytes": terrain_glb_byte_length},
                "sqliteWrite": {"elapsedMs": sqlite_write_elapsed_ms, "databaseBytes": database_byte_length, "rowCounts": database_row_counts},
                "databaseHash": {"elapsedMs": database_hash_elapsed_ms, "databaseBytes": database_byte_length},
                "manifestWrite": {"elapsedMs": manifest_write_elapsed_ms},
                "reportWrite": {"elapsedMs": elapsed_ms(report_started)}
            }
        }))?,
    )?;
    let bottleneck_stages = top_stage_summary(&stage_elapsed_ms, 3)
        .iter()
        .map(|stage| {
            format!(
                "{}={} ms ({}%)",
                stage.0,
                stage.1,
                percentage_of_total(stage.1, total_elapsed_ms)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!(
        "[citymodel] conversion finished: {total_elapsed_ms} ms; bottlenecks: {bottleneck_stages}; report: {}",
        report_path.display()
    );
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
                feature_type: FeatureType::Building,
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

fn extract_terrain(
    events: &[ParserEvent],
    source_file_id: i64,
) -> (Vec<RawTerrain>, Vec<ParsedTerrainTexture>) {
    #[derive(Clone)]
    struct Active {
        id: String,
        rings: Vec<RawTerrainRing>,
    }
    let mut active = Vec::<Active>::new();
    let mut output = Vec::new();
    let mut textures = Vec::new();
    for event in events {
        match event {
            ParserEvent::StartFeature {
                gml_id,
                feature_type: FeatureType::Terrain,
                ..
            } => active.push(Active {
                id: gml_id.clone(),
                rings: Vec::new(),
            }),
            ParserEvent::Coordinates(sequence) if sequence.is_linear_ring => {
                if let Some(terrain) = active.last_mut() {
                    let surface_id = sequence.surface_id.clone().unwrap_or_else(|| {
                        format!("{}:terrain-surface-{}", terrain.id, terrain.rings.len() + 1)
                    });
                    terrain.rings.push(RawTerrainRing {
                        surface_id,
                        values: sequence.values.clone(),
                        dimension: usize::from(sequence.dimension.unwrap_or(3)),
                        axis_order: sequence.axis_order,
                    });
                }
            }
            ParserEvent::TerrainTexture(texture) => textures.push(texture.clone()),
            ParserEvent::EndFeature => {
                if let Some(terrain) = active.pop() {
                    output.push(RawTerrain {
                        id: terrain.id,
                        source_file_id,
                        rings: terrain.rings,
                    });
                }
            }
            _ => {}
        }
    }
    (output, textures)
}

fn geographic_envelope_from_file(
    path: &Path,
) -> Result<Option<GeographicEnvelope>, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_file(path)?;
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut active_corner = None::<(&str, String)>;
    let mut lower = None;
    let mut upper = None;
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(element) => match element.local_name().as_ref() {
                "lowerCorner" => active_corner = Some(("lower", String::new())),
                "upperCorner" => active_corner = Some(("upper", String::new())),
                _ => {}
            },
            Event::Text(text) => {
                if let Some((_, value)) = &mut active_corner {
                    if value.len() + text.as_ref().len() > MAX_ENVELOPE_CORNER_TEXT_BYTES {
                        return Ok(None);
                    }
                    value.push_str(text.as_ref());
                }
            }
            Event::End(element) => {
                let local_name = element.local_name();
                let is_corner = matches!(local_name.as_ref(), "lowerCorner" | "upperCorner");
                if is_corner {
                    if let Some((kind, value)) = active_corner.take() {
                        let mut values = value.split_whitespace();
                        let north = values.next().and_then(|item| item.parse::<f64>().ok());
                        let east = values.next().and_then(|item| item.parse::<f64>().ok());
                        match (kind, north, east) {
                            ("lower", Some(north), Some(east)) => lower = Some((north, east)),
                            ("upper", Some(north), Some(east)) => upper = Some((north, east)),
                            _ => return Ok(None),
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(match (lower, upper) {
        (Some((south, west)), Some((north, east)))
            if south.is_finite()
                && west.is_finite()
                && north.is_finite()
                && east.is_finite()
                && south < north
                && west < east =>
        {
            Some(GeographicEnvelope {
                south,
                west,
                north,
                east,
            })
        }
        _ => None,
    })
}

fn safe_texture_image(
    input_root: &Path,
    uri: &str,
) -> Result<TerrainTexture, Box<dyn std::error::Error>> {
    let value = Path::new(uri);
    if uri.contains("://")
        || value.is_absolute()
        || value.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(format!("unsafe terrain texture URI: {uri}").into());
    }
    let root = fs::canonicalize(input_root)?;
    let candidate = fs::canonicalize(root.join(value))?;
    if !candidate.starts_with(&root) {
        return Err(format!("terrain texture escapes input root: {uri}").into());
    }
    let bytes = fs::read(candidate)?;
    if bytes.len() > MAX_TERRAIN_TEXTURE_BYTES {
        return Err("terrain texture exceeds 64 MiB limit".into());
    }
    let mime_type = image_mime_and_dimensions(&bytes)?;
    Ok(TerrainTexture { mime_type, bytes })
}

fn find_adjacent_map_texture(
    input_root: &Path,
    source_file: &Path,
    envelope: GeographicEnvelope,
) -> Result<TerrainMapTexture, String> {
    let source_name = source_file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("DEM source file name is not valid UTF-8")?;
    let map_directory = source_file
        .parent()
        .ok_or("DEM source file has no parent directory")?
        .join(format!("{source_name}_map"));
    if !map_directory.is_dir() {
        return Err(format!(
            "adjacent map directory does not exist: {}",
            map_directory.display()
        ));
    }
    let root = fs::canonicalize(input_root).map_err(|error| error.to_string())?;
    let map_root = fs::canonicalize(&map_directory).map_err(|error| error.to_string())?;
    if !map_root.starts_with(&root) {
        return Err("adjacent map directory escapes the input root".to_owned());
    }
    let candidates = fs::read_dir(&map_root)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("combined_map_mesh")
                            && name.to_ascii_lowercase().ends_with(".png")
                    })
        })
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(format!(
            "expected exactly one combined_map_mesh*.png in {}, found {}",
            map_directory.display(),
            candidates.len()
        ));
    }
    let candidate = fs::canonicalize(&candidates[0]).map_err(|error| error.to_string())?;
    if !candidate.starts_with(&map_root) {
        return Err("map texture resolves outside its adjacent map directory".to_owned());
    }
    let relative = candidate
        .strip_prefix(&root)
        .map_err(|_| "map texture resolves outside the input root")?
        .to_string_lossy()
        .replace('\\', "/");
    if fs::metadata(&candidate)
        .map_err(|error| error.to_string())?
        .len()
        > u64::try_from(MAX_TERRAIN_TEXTURE_BYTES).expect("texture byte limit fits in u64")
    {
        return Err("map texture exceeds 64 MiB limit".to_owned());
    }
    // Validate fallback imagery before it becomes part of a tile, so a corrupt
    // or oversized map skips only its terrain rather than aborting conversion.
    safe_texture_image(&root, &relative).map_err(|error| error.to_string())?;
    Ok(TerrainMapTexture {
        image_uri: relative,
        envelope,
    })
}

fn terrain_map_uvs(
    ring: &RawTerrainRing,
    envelope: GeographicEnvelope,
) -> Result<Vec<(f64, f64)>, String> {
    if ring.dimension < 2 || ring.values.len() < ring.dimension * 3 {
        return Err("surface has fewer than three geographic positions".to_owned());
    }
    let latitude_span = envelope.north - envelope.south;
    let longitude_span = envelope.east - envelope.west;
    if !latitude_span.is_finite()
        || !longitude_span.is_finite()
        || latitude_span <= 0.0
        || longitude_span <= 0.0
    {
        return Err("GML geographic boundedBy envelope is invalid".to_owned());
    }
    ring.values
        .chunks_exact(ring.dimension)
        .map(|coordinate| {
            let (latitude, longitude) = match ring.axis_order {
                AxisOrder::EastNorthUp => (coordinate[1], coordinate[0]),
                AxisOrder::NorthEastUp | AxisOrder::Unknown => (coordinate[0], coordinate[1]),
            };
            if !latitude.is_finite() || !longitude.is_finite() {
                return Err("surface has a non-finite geographic position".to_owned());
            }
            let u = (longitude - envelope.west) / longitude_span;
            // GML envelopes use increasing latitude northward, while raster rows increase down.
            let v = 1.0 - (latitude - envelope.south) / latitude_span;
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                return Err(
                    "surface position lies outside the GML geographic boundedBy envelope"
                        .to_owned(),
                );
            }
            Ok((u, v))
        })
        .collect()
}

fn image_mime_and_dimensions(bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    const MAX_DIMENSION: u32 = 16_384;
    if bytes.len() >= 24 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" && &bytes[12..16] == b"IHDR" {
        let width = u32::from_be_bytes(bytes[16..20].try_into()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into()?);
        if !valid_texture_dimensions(width, height, MAX_DIMENSION) {
            return Err("invalid PNG texture dimensions".into());
        }
        validate_png(bytes)?;
        return Ok("image/png".to_owned());
    }
    if bytes.len() >= 4 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        let mut index = 2;
        while index + 9 < bytes.len() {
            if bytes[index] != 0xff {
                index += 1;
                continue;
            }
            let marker = bytes[index + 1];
            index += 2;
            if marker == 0xd9 || marker == 0xda {
                break;
            }
            if index + 2 > bytes.len() {
                break;
            }
            let length = usize::from(u16::from_be_bytes([bytes[index], bytes[index + 1]]));
            if length < 2 || index + length > bytes.len() {
                break;
            }
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf)
                && length >= 7
            {
                let height = u32::from(u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]));
                let width = u32::from(u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]));
                if !valid_texture_dimensions(width, height, MAX_DIMENSION) {
                    return Err("invalid JPEG texture dimensions".into());
                }
                return Ok("image/jpeg".to_owned());
            }
            index += length;
        }
    }
    Err("terrain texture must be a PNG or JPEG with valid dimensions".into())
}

fn validate_png(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let buffer_size = reader
        .output_buffer_size()
        .ok_or("PNG decoded buffer size is unavailable")?;
    if u64::try_from(buffer_size)? > MAX_TERRAIN_TEXTURE_DECODED_RGBA_BYTES {
        return Err("PNG texture exceeds decoded memory limit".into());
    }
    let mut decoded_pixels = vec![0_u8; buffer_size];
    reader.next_frame(&mut decoded_pixels)?;
    Ok(())
}

fn valid_texture_dimensions(width: u32, height: u32, max_dimension: u32) -> bool {
    width != 0
        && height != 0
        && width <= max_dimension
        && height <= max_dimension
        && u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .is_some_and(|rgba_bytes| rgba_bytes <= MAX_TERRAIN_TEXTURE_DECODED_RGBA_BYTES)
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

#[allow(
    clippy::cast_precision_loss,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
fn write_terrain_tiles(
    output: &Path,
    generation_id: &str,
    terrain: Vec<RawTerrain>,
    texture_declarations: Vec<TerrainTextureDeclaration>,
    input_root: &Path,
    source_files: &[(i64, PathBuf, String, u64)],
    terrain_envelopes: &BTreeMap<i64, GeographicEnvelope>,
    issues: &mut Vec<ConversionIssue>,
) -> Result<Vec<TerrainTileOutput>, Box<dyn std::error::Error>> {
    let textures_by_surface = texture_declarations
        .into_iter()
        .map(|declaration| {
            (
                (
                    declaration.source_file_id,
                    declaration.texture.target_id.clone(),
                ),
                declaration.texture,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let source_paths = source_files
        .iter()
        .map(|(id, path, _, _)| (*id, path.as_path()))
        .collect::<BTreeMap<_, _>>();
    let mut fallback_by_source = BTreeMap::<i64, Result<TerrainMapTexture, String>>::new();
    let mut fallback_reported = BTreeSet::new();
    let mut grouped =
        BTreeMap::<TileId, Vec<(String, i64, RawTerrainRing, ParsedTerrainTexture)>>::new();
    for feature in terrain {
        for ring in feature.rings {
            let texture = if let Some(texture) =
                textures_by_surface.get(&(feature.source_file_id, ring.surface_id.clone()))
            {
                texture.clone()
            } else {
                let fallback = fallback_by_source
                    .entry(feature.source_file_id)
                    .or_insert_with(|| {
                        let Some(source_path) = source_paths.get(&feature.source_file_id) else {
                            return Err("terrain source file is unavailable".to_owned());
                        };
                        let Some(envelope) = terrain_envelopes.get(&feature.source_file_id) else {
                            return Err(
                                "GML geographic boundedBy envelope is unavailable".to_owned()
                            );
                        };
                        find_adjacent_map_texture(input_root, source_path, *envelope)
                    });
                match fallback {
                    Ok(map) => {
                        if fallback_reported.insert((feature.source_file_id, true)) {
                            issues.push(ConversionIssue {
                                source_file_id: feature.source_file_id,
                                building_id: None,
                                diagnostic: Diagnostic {
                                    kind: DiagnosticKind::UnsupportedElement,
                                    message: format!(
                                        "terrain map texture fallback used: {}",
                                        map.image_uri
                                    ),
                                },
                            });
                        }
                        match terrain_map_uvs(&ring, map.envelope) {
                            Ok(coordinates) => ParsedTerrainTexture {
                                target_id: ring.surface_id.clone(),
                                image_uri: map.image_uri.clone(),
                                coordinates,
                            },
                            Err(message) => {
                                issues.push(ConversionIssue {
                                    source_file_id: feature.source_file_id,
                                    building_id: Some(feature.id.clone()),
                                    diagnostic: Diagnostic {
                                        kind: DiagnosticKind::InvalidTexture,
                                        message: format!(
                                            "terrain map texture fallback excluded surface {}: {message}",
                                            ring.surface_id
                                        ),
                                    },
                                });
                                continue;
                            }
                        }
                    }
                    Err(message) => {
                        if fallback_reported.insert((feature.source_file_id, false)) {
                            issues.push(ConversionIssue {
                                source_file_id: feature.source_file_id,
                                building_id: None,
                                diagnostic: Diagnostic {
                                    kind: DiagnosticKind::InvalidTexture,
                                    message: format!(
                                        "terrain map texture fallback unavailable; terrain surfaces without explicit ParameterizedTexture were excluded: {message}"
                                    ),
                                },
                            });
                        }
                        continue;
                    }
                }
            };
            let Some(first) = ring.values.chunks(ring.dimension).next() else {
                continue;
            };
            if first.len() < 2 {
                continue;
            }
            let (north, east) = match ring.axis_order {
                AxisOrder::EastNorthUp => (first[1], first[0]),
                AxisOrder::NorthEastUp | AxisOrder::Unknown => (first[0], first[1]),
            };
            let point = web_mercator(east, north, first.get(2).copied().unwrap_or(0.0));
            grouped
                .entry(tile_for_point(point.x, point.y, DEFAULT_TILE_SIZE_METERS))
                .or_default()
                .push((feature.id.clone(), feature.source_file_id, ring, texture));
        }
    }
    let tile_count = grouped.len();
    let mut outputs = Vec::new();
    for (index, (grid, rings)) in grouped.into_iter().enumerate() {
        let id = format!("t_{}_{}_{}", grid.level, grid.x, grid.y);
        let origin = Point3 {
            x: grid.x as f64 * DEFAULT_TILE_SIZE_METERS,
            y: grid.y as f64 * DEFAULT_TILE_SIZE_METERS,
            z: 0.0,
        };
        let mut feature_names = BTreeSet::new();
        let mut loaded_textures = BTreeMap::<String, usize>::new();
        let mut textures = Vec::new();
        let mut source_triangles = Vec::<(String, i64, Vec<Point3>, Vec<(f64, f64)>, usize)>::new();
        let mut feature_sources = BTreeMap::new();
        for (feature_id, source_file_id, ring, texture) in rings {
            let points = ring
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
            let mut points = points;
            if points.first() == points.last() {
                points.pop();
            }
            let mut uvs = texture.coordinates;
            if uvs.len() > 1 && uvs.first() == uvs.last() {
                uvs.pop();
            }
            if points.len() < 3 || points.len() != uvs.len() {
                return Err(format!(
                    "terrain surface {} has {} positions but {} UVs",
                    ring.surface_id,
                    points.len(),
                    uvs.len()
                )
                .into());
            }
            let texture_index = if let Some(index) = loaded_textures.get(&texture.image_uri) {
                *index
            } else {
                let image = safe_texture_image(input_root, &texture.image_uri)?;
                let index = textures.len();
                textures.push(image);
                loaded_textures.insert(texture.image_uri, index);
                index
            };
            feature_names.insert(feature_id.clone());
            feature_sources.insert(feature_id.clone(), source_file_id);
            source_triangles.push((feature_id, source_file_id, points, uvs, texture_index));
        }
        let feature_ids = feature_names
            .into_iter()
            .enumerate()
            .map(|(index, feature_id)| Ok((feature_id, u16::try_from(index)?)))
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
        for (feature_id, _, points, uvs, texture_index) in source_triangles {
            for index in 1..points.len() - 1 {
                let positions = [points[0], points[index], points[index + 1]];
                let mut local_positions = positions;
                for point in &mut local_positions {
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
                triangles.push(TerrainTriangle {
                    positions: local_positions,
                    uvs: [uvs[0], uvs[index], uvs[index + 1]],
                    feature_id: feature_id.clone(),
                    texture_index,
                });
            }
        }
        let asset = write_terrain_glb(&TerrainGlbInput {
            tile_id: id.clone(),
            generation_id: generation_id.to_owned(),
            triangles: triangles.clone(),
            feature_ids: feature_ids.clone(),
            textures,
        })
        .map_err(|error| format!("terrain GLB write failed: {error:?}"))?;
        let glb_path = format!("terrain/{id}.glb");
        let metadata_path = format!("terrain/{id}.meta.json");
        let glb_output = output.join(&glb_path);
        if let Some(parent) = glb_output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(glb_output, &asset.bytes)?;
        let geographic = inverse_web_mercator(origin);
        let metadata_feature_ids = feature_ids.keys().cloned().collect::<Vec<_>>();
        let metadata_json = metadata::tile_metadata_json(&metadata::TileMetadataInput {
            generation_id,
            tile_id: &id,
            glb_path: &glb_path,
            glb_sha256: &asset.sha256,
            glb_byte_length: u64::try_from(asset.bytes.len())?,
            building_ids: &[],
            feature_ids: &metadata_feature_ids,
            feature_type: "terrain",
            tile_bounds: [
                origin.x,
                origin.y,
                origin.x + DEFAULT_TILE_SIZE_METERS,
                origin.y + DEFAULT_TILE_SIZE_METERS,
            ],
            content_bounds,
            projected_origin: [origin.x, origin.y, origin.z],
            geographic_origin: [geographic.0, geographic.1, origin.z],
            working_epsg: WORKING_EPSG,
            vertex_count: triangles.len() * 3,
            triangle_count: triangles.len(),
        });
        let metadata_output = metadata::write_json_under(output, &metadata_path, &metadata_json)?;
        let metadata_byte_length = usize::try_from(fs::metadata(&metadata_output)?.len())
            .map_err(|_| std::io::Error::other("terrain metadata exceeds supported size"))?;
        let feature_assignments = feature_ids
            .iter()
            .map(|(feature_id, local_feature_id)| {
                Ok((
                    feature_id.clone(),
                    *feature_sources
                        .get(feature_id)
                        .ok_or("missing terrain source file")?,
                    *local_feature_id,
                ))
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        eprintln!(
            "[citymodel] terrain tile [{}/{}] finished: {} ({} triangles)",
            index + 1,
            tile_count,
            id,
            triangles.len()
        );
        outputs.push(TerrainTileOutput {
            id,
            feature_assignments,
            glb_path,
            metadata_path,
            metadata_sha256: sha256_file(&metadata_output)?,
            metadata_byte_length,
            glb_sha256: asset.sha256,
            glb_byte_length: asset.bytes.len(),
            bounds: [
                origin.x,
                origin.y,
                origin.x + DEFAULT_TILE_SIZE_METERS,
                origin.y + DEFAULT_TILE_SIZE_METERS,
            ],
            content_bounds,
            origin,
            triangle_count: triangles.len(),
        });
    }
    Ok(outputs)
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
    let tile_count = grouped.len();
    let mut outputs = Vec::new();
    let mut assignments = Vec::new();
    for (index, (grid, mut buildings)) in grouped.into_iter().enumerate() {
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
            feature_ids: &building_ids,
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
        let metadata_byte_length = usize::try_from(fs::metadata(&metadata_output)?.len())
            .map_err(|_| std::io::Error::other("tile metadata exceeds supported size"))?;
        let metadata_sha256 = sha256_file(&metadata_output)?;
        eprintln!(
            "[citymodel] building tile [{}/{}] finished: {} ({} buildings, {} triangles)",
            index + 1,
            tile_count,
            id,
            building_ids.len(),
            triangle_count
        );
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
    terrain_tiles: &[TerrainTileOutput],
    assignments: &[BuildingAssignment],
    issues: &[ConversionIssue],
    origin: Point3,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = create_database(path)?;
    connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
    let transaction = connection.transaction()?;
    let placeholder = "0".repeat(64);
    transaction.execute("INSERT INTO dataset_metadata (dataset_id, schema_version, generation_id, generated_at, generator_name, generator_version, source_crs_epsg, source_crs_wkt, working_crs_epsg, working_crs_wkt, vertical_crs_epsg, vertical_reference_type, axis_order_json, dataset_origin_latitude, dataset_origin_longitude, dataset_origin_height, dataset_origin_geographic_epsg, dataset_origin_x, dataset_origin_y, dataset_origin_z, manifest_sha256, database_sha256, conversion_config_json, license_json) VALUES (?1, '1.0.0', ?2, '1970-01-01T00:00:00Z', 'citymodel', '0.1.0-dev', 6697, NULL, 3857, NULL, NULL, 'source-defined', '[\"latitude\",\"longitude\",\"height\"]', 0.0, 0.0, 0.0, 4326, ?3, ?4, ?5, ?6, ?6, '{}', '{}')", params![dataset_id, generation_id, origin.x, origin.y, origin.z, placeholder])?;
    for (id, file, sha256, length) in source_files {
        transaction.execute("INSERT INTO source_files (source_file_id, dataset_id, relative_path, sha256, byte_length) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, dataset_id, file.file_name().and_then(|name| name.to_str()).unwrap_or("input.gml"), sha256, length])?;
    }
    for tile in tiles {
        transaction.execute("INSERT INTO tiles (tile_id, dataset_id, generation_id, glb_relative_path, metadata_relative_path, glb_sha256, glb_byte_length, origin_latitude, origin_longitude, origin_height, origin_geographic_epsg, origin_x, origin_y, origin_z, tile_min_x, tile_min_y, tile_max_x, tile_max_y, content_min_x, content_min_y, content_min_z, content_max_x, content_max_y, content_max_z, projected_to_local_matrix_json, building_count, vertex_count, triangle_count, primitive_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0.0, 0.0, 0.0, 4326, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, '[]', ?21, 0, ?22, 1)", params![tile.id, dataset_id, generation_id, tile.glb_path, tile.metadata_path, tile.glb_sha256, tile.glb_byte_length as i64, tile.origin.x, tile.origin.y, tile.origin.z, tile.bounds[0], tile.bounds[1], tile.bounds[2], tile.bounds[3], tile.content_bounds[0], tile.content_bounds[1], tile.content_bounds[2], tile.content_bounds[3], tile.content_bounds[4], tile.content_bounds[5], tile.building_ids.len() as i64, tile.triangle_count as i64])?;
        transaction.execute("INSERT INTO tile_contents (tile_id, feature_type, metadata_relative_path, metadata_sha256, metadata_byte_length, glb_relative_path, glb_sha256, glb_byte_length) VALUES (?1, 'building', ?2, ?3, ?4, ?5, ?6, ?7)", params![tile.id, tile.metadata_path, tile.metadata_sha256, tile.metadata_byte_length as i64, tile.glb_path, tile.glb_sha256, tile.glb_byte_length as i64])?;
    }
    for tile in terrain_tiles {
        if !tiles
            .iter()
            .any(|building_tile| building_tile.id == tile.id)
        {
            let geographic = inverse_web_mercator(tile.origin);
            transaction.execute("INSERT INTO tiles (tile_id, dataset_id, generation_id, glb_relative_path, metadata_relative_path, glb_sha256, glb_byte_length, origin_latitude, origin_longitude, origin_height, origin_geographic_epsg, origin_x, origin_y, origin_z, tile_min_x, tile_min_y, tile_max_x, tile_max_y, content_min_x, content_min_y, content_min_z, content_max_x, content_max_y, content_max_z, projected_to_local_matrix_json, building_count, vertex_count, triangle_count, primitive_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 4326, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, '[]', 0, 0, ?24, 1)", params![tile.id, dataset_id, generation_id, tile.glb_path, tile.metadata_path, tile.glb_sha256, tile.glb_byte_length as i64, geographic.0, geographic.1, tile.origin.z, tile.origin.x, tile.origin.y, tile.origin.z, tile.bounds[0], tile.bounds[1], tile.bounds[2], tile.bounds[3], tile.content_bounds[0], tile.content_bounds[1], tile.content_bounds[2], tile.content_bounds[3], tile.content_bounds[4], tile.content_bounds[5], tile.triangle_count as i64])?;
        }
        transaction.execute("INSERT INTO tile_contents (tile_id, feature_type, metadata_relative_path, metadata_sha256, metadata_byte_length, glb_relative_path, glb_sha256, glb_byte_length) VALUES (?1, 'terrain', ?2, ?3, ?4, ?5, ?6, ?7)", params![tile.id, tile.metadata_path, tile.metadata_sha256, tile.metadata_byte_length as i64, tile.glb_path, tile.glb_sha256, tile.glb_byte_length as i64])?;
        for (feature_id, source_file_id, local_feature_id) in &tile.feature_assignments {
            let canonical = format!("{dataset_id}::{feature_id}");
            transaction.execute("INSERT INTO features (feature_id, canonical_feature_id, feature_type, gml_id, id_source, id_is_synthetic, source_file_id) VALUES (?1, ?2, 'terrain', ?1, 'gml', 0, ?3)", params![feature_id, canonical, source_file_id])?;
            transaction.execute("INSERT INTO feature_tile_mappings (tile_id, feature_type, local_feature_id, feature_id) VALUES (?1, 'terrain', ?2, ?3)", params![tile.id, i64::from(*local_feature_id), feature_id])?;
        }
    }
    for assignment in assignments {
        let canonical = format!("{dataset_id}::{}", assignment.building_id);
        insert_building(
            &transaction,
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
        transaction.execute("UPDATE buildings SET tile_id=?1, local_feature_id=?2, lod_used=?3, centroid_x=?4, centroid_y=?5, attributes_json=?6 WHERE building_id=?7", params![assignment.tile_id, i64::from(assignment.feature_id), i64::from(assignment.lod_used), assignment.centroid.x, assignment.centroid.y, attributes_json, assignment.building_id])?;
        transaction.execute("INSERT INTO tile_features (tile_id, local_feature_id, building_id, building_part_id) VALUES (?1, ?2, ?3, NULL)", params![assignment.tile_id, i64::from(assignment.feature_id), assignment.building_id])?;
        transaction.execute("INSERT INTO features (feature_id, canonical_feature_id, feature_type, gml_id, id_source, id_is_synthetic, source_file_id) VALUES (?1, ?2, 'building', ?1, 'gml', 0, ?3)", params![assignment.building_id, canonical, assignment.source_file_id])?;
        transaction.execute("INSERT INTO feature_tile_mappings (tile_id, feature_type, local_feature_id, feature_id) VALUES (?1, 'building', ?2, ?3)", params![assignment.tile_id, i64::from(assignment.feature_id), assignment.building_id])?;
        insert_attributes(
            &transaction,
            &assignment.building_id,
            &assignment.attributes,
        )?;
        insert_feature_attributes(
            &transaction,
            &assignment.building_id,
            &assignment.attributes,
        )?;
    }
    for issue in issues {
        transaction.execute(
            "INSERT INTO conversion_issues (source_file_id, building_id, gml_id, severity, error_code, message, element_path, repaired, exclusion_reason, occurred_at) VALUES (?1, ?2, ?2, 'warn', ?3, ?4, NULL, 0, NULL, '1970-01-01T00:00:00Z')",
            params![issue.source_file_id, issue.building_id, format!("{:?}", issue.diagnostic.kind), issue.diagnostic.message],
        )?;
    }
    transaction.commit()?;
    verify_integrity(&connection)?;
    Ok(())
}

fn insert_attributes(
    transaction: &rusqlite::Transaction<'_>,
    building_id: &str,
    attributes: &[BuildingAttribute],
) -> rusqlite::Result<()> {
    let mut ordinals = BTreeMap::<(String, String), i64>::new();
    let mut statement = transaction.prepare_cached(
        "INSERT INTO building_attributes (building_id, namespace_uri, attribute_path, attribute_key, ordinal, value_type, value_text, value_real, value_integer, value_boolean, value_datetime, uom, code_space, nil_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, ?9, ?10, ?11)",
    )?;
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
        statement.execute(params![
            building_id,
            attribute.namespace_uri,
            attribute.attribute_path,
            attribute.attribute_key,
            *ordinal,
            value_type,
            value_text,
            value_real,
            attribute.uom,
            attribute.code_space,
            attribute.nil_reason
        ])?;
        *ordinal += 1;
    }
    Ok(())
}

fn insert_feature_attributes(
    transaction: &rusqlite::Transaction<'_>,
    feature_id: &str,
    attributes: &[BuildingAttribute],
) -> rusqlite::Result<()> {
    let mut ordinals = BTreeMap::<(String, String), i64>::new();
    let mut statement = transaction.prepare_cached(
        "INSERT INTO feature_attributes (feature_id, namespace_uri, attribute_path, attribute_key, ordinal, value_type, value_text, value_real, value_integer, value_boolean, value_datetime, uom, code_space, nil_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, ?9, ?10, ?11)",
    )?;
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
        statement.execute(params![
            feature_id,
            attribute.namespace_uri,
            attribute.attribute_path,
            attribute.attribute_key,
            *ordinal,
            value_type,
            value_text,
            value_real,
            attribute.uom,
            attribute.code_space,
            attribute.nil_reason
        ])?;
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
    terrain_tiles: &[TerrainTileOutput],
    origin: Point3,
    database_sha256: &str,
    max_lod: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let geographic = inverse_web_mercator(origin);
    let input_files = source_files.iter().map(|(_, path, sha256, _)| json!({"path":path.file_name().and_then(|name| name.to_str()).unwrap_or("input.gml"),"sha256":sha256})).collect::<Vec<_>>();
    let mut contents = BTreeMap::<String, Vec<serde_json::Value>>::new();
    for tile in tiles {
        contents.entry(tile.id.clone()).or_default().push(json!({"featureType":"building","metadata":tile.metadata_path,"sha256":tile.metadata_sha256,"byteLength":tile.metadata_byte_length}));
    }
    for tile in terrain_tiles {
        contents.entry(tile.id.clone()).or_default().push(json!({"featureType":"terrain","metadata":tile.metadata_path,"sha256":tile.metadata_sha256,"byteLength":tile.metadata_byte_length}));
    }
    let items = contents
        .into_iter()
        .map(|(tile_id, contents)| {
            let metadata = contents
                .first()
                .and_then(|item| item["metadata"].as_str())
                .unwrap_or_default();
            json!({"tileId":tile_id,"metadata":metadata,"contents":contents})
        })
        .collect::<Vec<_>>();
    let manifest = json!({"schemaVersion":"1.0.0","datasetId":dataset_id,"generationId":generation_id,"generatedAt":"1970-01-01T00:00:00Z","generator":{"name":"citymodel","version":"0.1.0-dev"},"source":{"format":"CityGML","profile":"PLATEAU","citygmlVersion":"2.0","files":source_files.len(),"inputFiles":input_files,"conversionConfiguration":{"lod":max_lod,"maxLod":max_lod,"lodSelection":"highest-available-at-or-below-max-lod","tileSizeMetres":DEFAULT_TILE_SIZE_METERS,"workingCrs":"EPSG:3857"}},"coordinateReference":{"sourceCrs":{"epsg":6697,"wkt":null,"axisOrder":["latitude","longitude","height"]},"workingCrs":{"epsg":WORKING_EPSG,"wkt":null,"axisOrder":["easting","northing","height"],"unit":"metre"},"verticalReference":{"type":"source-defined","epsg":null,"geoidModel":null}},"datasetOrigin":{"geographic":{"latitude":geographic.0,"longitude":geographic.1,"height":origin.z,"epsg":4326},"projected":{"x":origin.x,"y":origin.y,"z":origin.z,"epsg":WORKING_EPSG}},"tiling":{"scheme":"projected-grid","defaultTileSizeMetres":DEFAULT_TILE_SIZE_METERS,"buildingAssignment":"representative-point","geometryClipping":false},"modelProfile":{"lod":max_lod,"textures":!terrain_tiles.is_empty(),"compression":null,"featureIdSemantic":"_FEATURE_ID_0","featureIdComponentType":"UNSIGNED_SHORT"},"database":{"path":"citymodel.sqlite","sha256":database_sha256},"tiles":{"indexType":"inline","items":items}});
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

fn database_row_counts(
    path: &Path,
) -> Result<BTreeMap<&'static str, i64>, Box<dyn std::error::Error>> {
    const TABLES: [&str; 10] = [
        "source_files",
        "tiles",
        "tile_contents",
        "buildings",
        "building_attributes",
        "features",
        "feature_attributes",
        "feature_tile_mappings",
        "tile_features",
        "conversion_issues",
    ];
    let connection = rusqlite::Connection::open(path)?;
    TABLES
        .into_iter()
        .map(|table| {
            let count =
                connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?;
            Ok((table, count))
        })
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .map_err(Into::into)
}

fn elapsed_ms(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

#[allow(clippy::too_many_arguments)]
fn conversion_summary(
    total_elapsed_ms: u128,
    stage_elapsed_ms: &[(&str, u128)],
    input_files: &[InputFileBreakdown],
    raw_building_count: usize,
    raw_terrain_count: usize,
    prepared_building_count: usize,
    building_tile_count: usize,
    terrain_tile_count: usize,
    building_triangle_count: usize,
    terrain_triangle_count: usize,
    building_glb_byte_length: usize,
    terrain_glb_byte_length: usize,
    database_byte_length: u64,
    diagnostics: usize,
) -> serde_json::Value {
    let total_input_bytes = input_files.iter().map(|file| file.byte_length).sum::<u64>();
    let stage_timings = top_stage_summary(stage_elapsed_ms, stage_elapsed_ms.len())
        .into_iter()
        .map(|(name, elapsed_ms)| {
            json!({
                "name": name,
                "elapsedMs": elapsed_ms,
                "percentOfTotal": percentage_of_total(elapsed_ms, total_elapsed_ms),
            })
        })
        .collect::<Vec<_>>();
    let slowest_input_files = input_file_summary(input_files, |left, right| {
        right
            .parsing_elapsed_ms
            .cmp(&left.parsing_elapsed_ms)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    let largest_input_files = input_file_summary(input_files, |left, right| {
        right
            .byte_length
            .cmp(&left.byte_length)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    json!({
        "totalElapsedMs": total_elapsed_ms,
        "stageTimings": stage_timings,
        "input": {
            "fileCount": input_files.len(),
            "byteLength": total_input_bytes,
        },
        "features": {
            "rawBuildings": raw_building_count,
            "rawTerrainFeatures": raw_terrain_count,
            "preparedBuildings": prepared_building_count,
        },
        "outputs": {
            "buildingTiles": building_tile_count,
            "terrainTiles": terrain_tile_count,
            "totalTiles": building_tile_count + terrain_tile_count,
            "buildingTriangles": building_triangle_count,
            "terrainTriangles": terrain_triangle_count,
            "totalTriangles": building_triangle_count + terrain_triangle_count,
            "buildingGlbBytes": building_glb_byte_length,
            "terrainGlbBytes": terrain_glb_byte_length,
            "totalGlbBytes": building_glb_byte_length + terrain_glb_byte_length,
            "sqliteBytes": database_byte_length,
        },
        "diagnostics": diagnostics,
        "slowestInputFiles": slowest_input_files,
        "largestInputFiles": largest_input_files,
    })
}

fn top_stage_summary<'a>(
    stage_elapsed_ms: &'a [(&'a str, u128)],
    limit: usize,
) -> Vec<(&'a str, u128)> {
    let mut stages = stage_elapsed_ms.to_vec();
    stages.sort_unstable_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
    stages.truncate(limit);
    stages
}

fn percentage_of_total(elapsed_ms: u128, total_elapsed_ms: u128) -> String {
    let tenths_of_percent = elapsed_ms
        .saturating_mul(1_000)
        .checked_div(total_elapsed_ms)
        .unwrap_or_default();
    format!("{}.{:01}", tenths_of_percent / 10, tenths_of_percent % 10)
}

fn input_file_summary(
    input_files: &[InputFileBreakdown],
    mut compare: impl FnMut(&InputFileBreakdown, &InputFileBreakdown) -> std::cmp::Ordering,
) -> Vec<serde_json::Value> {
    let mut files = input_files.iter().collect::<Vec<_>>();
    files.sort_unstable_by(|left, right| compare(left, right));
    files
        .into_iter()
        .take(5)
        .map(|file| {
            json!({
                "relativePath": file.relative_path,
                "byteLength": file.byte_length,
                "parsingElapsedMs": file.parsing_elapsed_ms,
                "featureCount": file.raw_buildings + file.raw_terrain_features,
                "rawBuildings": file.raw_buildings,
                "rawTerrainFeatures": file.raw_terrain_features,
                "terrainTextureDeclarations": file.terrain_texture_declarations,
                "diagnostics": file.diagnostics,
            })
        })
        .collect()
}

fn input_root(input: &Path) -> Result<&Path, Box<dyn std::error::Error>> {
    if input.is_file() {
        input.parent().ok_or("input file has no parent".into())
    } else {
        Ok(input)
    }
}

fn input_relative_path<'a>(path: &'a Path, input_root: &Path) -> &'a Path {
    path.strip_prefix(input_root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
}

fn status_stage_started(stage: &str) {
    eprintln!("[citymodel] {stage}: started");
}

fn status_stage_finished(stage: &str, elapsed_ms: u128) {
    eprintln!("[citymodel] {stage}: finished ({elapsed_ms} ms)");
}

fn stage_error(stage: &str, error: impl std::fmt::Display) -> Box<dyn std::error::Error> {
    std::io::Error::other(format!("{stage} failed: {error}")).into()
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

    fn tiny_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0; 16]).unwrap();
        }
        bytes
    }
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
    fn conversion_summary_orders_bottlenecks_and_input_files_stably() {
        let input_files = vec![
            InputFileBreakdown {
                relative_path: "z/slow.gml".to_owned(),
                byte_length: 10,
                parsing_elapsed_ms: 50,
                raw_buildings: 1,
                raw_terrain_features: 0,
                terrain_texture_declarations: 0,
                diagnostics: 0,
            },
            InputFileBreakdown {
                relative_path: "a/slow.gml".to_owned(),
                byte_length: 20,
                parsing_elapsed_ms: 50,
                raw_buildings: 0,
                raw_terrain_features: 2,
                terrain_texture_declarations: 1,
                diagnostics: 3,
            },
            InputFileBreakdown {
                relative_path: "middle.gml".to_owned(),
                byte_length: 30,
                parsing_elapsed_ms: 10,
                raw_buildings: 0,
                raw_terrain_features: 0,
                terrain_texture_declarations: 0,
                diagnostics: 0,
            },
        ];
        let summary = conversion_summary(
            200,
            &[
                ("sqliteWrite", 70),
                ("parsingAndExtraction", 70),
                ("manifestWrite", 5),
            ],
            &input_files,
            1,
            2,
            1,
            1,
            1,
            10,
            20,
            100,
            200,
            300,
            3,
        );

        assert_eq!(summary["totalElapsedMs"], 200);
        assert_eq!(summary["input"]["fileCount"], 3);
        assert_eq!(summary["input"]["byteLength"], 60);
        assert_eq!(summary["outputs"]["totalTriangles"], 30);
        assert_eq!(summary["outputs"]["totalGlbBytes"], 300);
        assert_eq!(summary["diagnostics"], 3);
        assert_eq!(
            summary["stageTimings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|stage| stage["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["parsingAndExtraction", "sqliteWrite", "manifestWrite"]
        );
        assert_eq!(
            summary["slowestInputFiles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|file| file["relativePath"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["a/slow.gml", "z/slow.gml", "middle.gml"]
        );
        assert_eq!(
            summary["largestInputFiles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|file| file["relativePath"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["middle.gml", "a/slow.gml", "z/slow.gml"]
        );
        assert_eq!(summary["stageTimings"][0]["percentOfTotal"], "35.0");
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn converts_textured_tin_relief_to_independent_terrain_content() {
        let root =
            std::env::temp_dir().join(format!("citymodel-terrain-e2e-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let input = root.join("dataset");
        fs::create_dir_all(input.join("udx/dem")).unwrap();
        fs::create_dir_all(input.join("udx/bldg")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../citymodel-citygml/tests/fixtures/plateau-lod1-small.gml"),
            input.join("udx/bldg/building.gml"),
        )
        .unwrap();
        let terrain_gml = input.join("udx/dem/terrain.gml");
        fs::write(&terrain_gml, r##"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:dem="http://www.opengis.net/citygml/relief/2.0" xmlns:app="http://www.opengis.net/citygml/appearance/2.0" xmlns:gml="http://www.opengis.net/gml"><dem:ReliefFeature gml:id="terrain-1"><dem:reliefComponent><dem:TINRelief><gml:Triangle gml:id="surface-1"><gml:LinearRing gml:id="ring-1" srsName="urn:ogc:def:crs:EPSG::6697"><gml:posList>35 139 0 35 139.1 0 35.1 139 0 35 139 0</gml:posList></gml:LinearRing></gml:Triangle></dem:TINRelief></dem:reliefComponent></dem:ReliefFeature><app:ParameterizedTexture><app:imageURI>terrain.png</app:imageURI><app:target uri="#ring-1"><app:TexCoordList><app:textureCoordinates>0 0 1 0 0 1 0 0</app:textureCoordinates></app:TexCoordList></app:target></app:ParameterizedTexture></core:CityModel>"##).unwrap();
        fs::write(input.join("terrain.png"), tiny_png()).unwrap();
        fs::create_dir_all(input.join("udx/dem/terrain.gml_map")).unwrap();
        fs::write(
            input.join("udx/dem/terrain.gml_map/combined_map_mesh0.png"),
            tiny_png(),
        )
        .unwrap();
        fs::write(
            input.join("udx/dem/terrain.gml_map/combined_map_mesh1.png"),
            tiny_png(),
        )
        .unwrap();
        let output = root.join("output");
        convert(&input, &output, Mode::Strict, 1).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("dataset.manifest.json")).unwrap())
                .unwrap();
        let contents = manifest["tiles"]["items"][0]["contents"]
            .as_array()
            .unwrap();
        let terrain = contents
            .iter()
            .find(|content| content["featureType"] == "terrain")
            .unwrap();
        let metadata_path = terrain["metadata"].as_str().unwrap();
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join(metadata_path)).unwrap()).unwrap();
        assert_eq!(metadata["content"]["featureType"], "terrain");
        assert_eq!(metadata["features"]["items"][0]["featureId"], "terrain-1");
        let glb = fs::read(output.join(metadata["content"]["glb"].as_str().unwrap())).unwrap();
        assert!(glb.windows(10).any(|value| value == b"TEXCOORD_0"));
        assert!(glb.windows(9).any(|value| value == b"image/png"));
        let database = rusqlite::Connection::open(output.join("citymodel.sqlite")).unwrap();
        let mappings = database
            .prepare("SELECT feature_type, local_feature_id, feature_id FROM feature_tile_mappings ORDER BY feature_type")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        assert_eq!(
            mappings,
            vec![
                ("building".to_owned(), 0, "sample-building-1".to_owned()),
                ("terrain".to_owned(), 0, "terrain-1".to_owned())
            ]
        );
        let content_types = database
            .prepare("SELECT feature_type FROM tile_contents ORDER BY feature_type")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        assert_eq!(content_types, vec!["building", "terrain"]);
        assert_eq!(
            database
                .query_row("SELECT COUNT(*) FROM conversion_issues", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terrain_map_uvs_cover_geographic_corners_with_inverted_v_axis() {
        let ring = RawTerrainRing {
            surface_id: "surface".to_owned(),
            values: vec![35.0, 139.0, 0.0, 36.0, 140.0, 0.0, 35.0, 140.0, 0.0],
            dimension: 3,
            axis_order: AxisOrder::NorthEastUp,
        };
        assert_eq!(
            terrain_map_uvs(
                &ring,
                GeographicEnvelope {
                    south: 35.0,
                    west: 139.0,
                    north: 36.0,
                    east: 140.0,
                },
            )
            .unwrap(),
            vec![(0.0, 1.0), (1.0, 0.0), (1.0, 1.0)]
        );
    }

    #[test]
    fn map_texture_fallback_embeds_adjacent_png_and_synthesizes_surface_id() {
        let root = std::env::temp_dir().join(format!(
            "citymodel-terrain-map-fallback-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let input = root.join("dataset");
        let dem = input.join("udx/dem");
        fs::create_dir_all(dem.join("terrain.gml_map")).unwrap();
        let terrain_gml = dem.join("terrain.gml");
        fs::write(&terrain_gml, r#"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:dem="http://www.opengis.net/citygml/relief/2.0" xmlns:gml="http://www.opengis.net/gml"><gml:boundedBy><gml:Envelope srsName="urn:ogc:def:crs:EPSG::6697"><gml:lowerCorner>35 139</gml:lowerCorner><gml:upperCorner>36 140</gml:upperCorner></gml:Envelope></gml:boundedBy><dem:ReliefFeature gml:id="terrain-1"><dem:reliefComponent><dem:TINRelief><gml:Triangle><gml:LinearRing srsName="urn:ogc:def:crs:EPSG::6697"><gml:posList>35 139 0 36 140 0 35 140 0</gml:posList></gml:LinearRing></gml:Triangle></dem:TINRelief></dem:reliefComponent></dem:ReliefFeature></core:CityModel>"#).unwrap();
        fs::write(
            dem.join("terrain.gml_map/combined_map_mesh0_v0_p0.png"),
            tiny_png(),
        )
        .unwrap();
        let report = parse_file(
            hash_input_file(&terrain_gml).unwrap(),
            InputLimits::default(),
        );
        let (terrain, _) = extract_terrain(&report.events, 1);
        assert_eq!(
            terrain[0].rings[0].surface_id,
            "terrain-1:terrain-surface-1"
        );
        let output = root.join("output");
        convert(&input, &output, Mode::Strict, 1).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("dataset.manifest.json")).unwrap())
                .unwrap();
        let terrain = manifest["tiles"]["items"][0]["contents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|content| content["featureType"] == "terrain")
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_slice(
            &fs::read(output.join(terrain["metadata"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let glb = fs::read(output.join(metadata["content"]["glb"].as_str().unwrap())).unwrap();
        assert!(glb.windows(10).any(|value| value == b"TEXCOORD_0"));
        assert!(glb.windows(9).any(|value| value == b"image/png"));
        let database = rusqlite::Connection::open(output.join("citymodel.sqlite")).unwrap();
        let diagnostic: String = database
            .query_row("SELECT message FROM conversion_issues", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(diagnostic.starts_with("terrain map texture fallback used:"));
        drop(database);
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("conversion.report.json")).unwrap())
                .unwrap();
        assert_eq!(
            report["terrainTextureFallbacks"].as_array().unwrap().len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_or_corrupt_adjacent_map_skips_only_untextured_terrain() {
        let root = std::env::temp_dir().join(format!(
            "citymodel-terrain-map-missing-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let input = root.join("dataset");
        fs::create_dir_all(input.join("udx/dem")).unwrap();
        fs::create_dir_all(input.join("udx/bldg")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../citymodel-citygml/tests/fixtures/plateau-lod1-small.gml"),
            input.join("udx/bldg/building.gml"),
        )
        .unwrap();
        let terrain_gml = input.join("udx/dem/terrain.gml");
        fs::write(&terrain_gml, r#"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:dem="http://www.opengis.net/citygml/relief/2.0" xmlns:gml="http://www.opengis.net/gml"><gml:boundedBy><gml:Envelope><gml:lowerCorner>35 139</gml:lowerCorner><gml:upperCorner>36 140</gml:upperCorner></gml:Envelope></gml:boundedBy><dem:ReliefFeature gml:id="terrain-1"><gml:Triangle><gml:LinearRing><gml:posList>35 139 0 36 140 0 35 140 0</gml:posList></gml:LinearRing></gml:Triangle></dem:ReliefFeature></core:CityModel>"#).unwrap();
        let envelope = GeographicEnvelope {
            south: 35.0,
            west: 139.0,
            north: 36.0,
            east: 140.0,
        };
        assert!(find_adjacent_map_texture(&input, &terrain_gml, envelope).is_err());
        let map = input.join("udx/dem/terrain.gml_map");
        fs::create_dir_all(&map).unwrap();
        fs::write(map.join("combined_map_mesh0.png"), b"not a PNG").unwrap();
        let output = root.join("output");
        convert(&input, &output, Mode::Strict, 1).unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("conversion.report.json")).unwrap())
                .unwrap();
        assert_eq!(report["terrainTiles"], 0);
        assert!(report["summary"]["diagnostics"].as_u64().unwrap() >= 1);
        assert!(
            report["terrainTextureFallbacks"][0]["message"]
                .as_str()
                .unwrap()
                .contains("fallback unavailable")
        );
        assert!(
            report["terrainTextureFallbacks"][0]["message"]
                .as_str()
                .unwrap()
                .contains("terrain texture must be a PNG or JPEG")
        );
        let database = rusqlite::Connection::open(output.join("citymodel.sqlite")).unwrap();
        let diagnostic: String = database
            .query_row("SELECT message FROM conversion_issues", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(diagnostic.contains("fallback unavailable"));
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unsafe_and_invalid_terrain_textures() {
        let root = std::env::temp_dir();
        assert!(safe_texture_image(&root, "../escape.png").is_err());
        assert!(safe_texture_image(&root, "https://example.test/terrain.png").is_err());
        assert!(image_mime_and_dimensions(b"not an image").is_err());
        let mut header_only = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        header_only.extend(2_u32.to_be_bytes());
        header_only.extend(2_u32.to_be_bytes());
        header_only.extend([8, 2, 0, 0, 0]);
        assert!(image_mime_and_dimensions(&header_only).is_err());
        let mut decompression_bomb = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        decompression_bomb.extend(16_384_u32.to_be_bytes());
        decompression_bomb.extend(16_384_u32.to_be_bytes());
        decompression_bomb.extend([8, 2, 0, 0, 0]);
        assert!(image_mime_and_dimensions(&decompression_bomb).is_err());
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn converts_lod1_fixture_to_a_unity_dataset() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../citymodel-citygml/tests/fixtures/plateau-lod1-small.gml");
        let output = std::env::temp_dir().join(format!("citymodel-cli-e2e-{}", std::process::id()));
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&output).unwrap();
        convert(&fixture, &output, Mode::Strict, 1).unwrap();

        let conversion_report: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("conversion.report.json")).unwrap())
                .unwrap();
        assert!(conversion_report["totalElapsedMs"].is_u64());
        let summary = &conversion_report["summary"];
        assert_eq!(summary["input"]["fileCount"], 1);
        assert_eq!(summary["features"]["rawBuildings"], 1);
        assert_eq!(summary["outputs"]["buildingTiles"], 1);
        assert!(summary["outputs"]["sqliteBytes"].as_u64().unwrap() > 0);
        assert_eq!(summary["slowestInputFiles"].as_array().unwrap().len(), 1);
        assert_eq!(summary["largestInputFiles"].as_array().unwrap().len(), 1);
        assert_eq!(conversion_report["stages"].as_object().unwrap().len(), 9);
        assert_eq!(
            conversion_report["stages"]["inputDiscoveryAndHashing"]["inputFiles"],
            1
        );
        let input_files = conversion_report["inputFiles"].as_array().unwrap();
        assert_eq!(input_files.len(), 1);
        assert_eq!(
            input_files[0]["relativePath"],
            fixture.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(
            input_files[0]["byteLength"],
            fs::metadata(&fixture).unwrap().len()
        );
        assert_eq!(
            conversion_report["stages"]["parsingAndExtraction"]["inputBytes"],
            fs::metadata(&fixture).unwrap().len()
        );
        assert_eq!(input_files[0]["rawBuildings"], 1);
        assert_eq!(
            conversion_report["stages"]["parsingAndExtraction"]["rawBuildings"],
            1
        );
        assert_eq!(
            conversion_report["stages"]["buildingGeometryPreparation"]["preparedBuildings"],
            1
        );
        assert!(
            conversion_report["stages"]["buildingGlbTiles"]["glbBytes"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            conversion_report["stages"]["sqliteWrite"]["databaseBytes"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(
            conversion_report["stages"]["sqliteWrite"]["rowCounts"]["buildings"],
            1
        );

        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("dataset.manifest.json")).unwrap())
                .unwrap();
        let metadata = manifest["tiles"]["items"][0]["metadata"].as_str().unwrap();
        let content_index = &manifest["tiles"]["items"][0]["contents"][0];
        assert_eq!(content_index["featureType"], "building");
        assert_eq!(content_index["metadata"], metadata);
        assert_eq!(
            usize::try_from(content_index["byteLength"].as_u64().unwrap()).unwrap(),
            usize::try_from(fs::metadata(output.join(metadata)).unwrap().len()).unwrap()
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
