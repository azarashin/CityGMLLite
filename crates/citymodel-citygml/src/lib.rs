//! Safe, streaming `CityGML` input boundary.
//!
//! The reader deliberately emits a small, normalized event stream.  Building
//! identity and geometry construction belong to the following pipeline stages.

use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
};

use quick_xml::{
    XmlVersion,
    events::{BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
};
use sha2::{Digest, Sha256};

/// Name used by diagnostics to identify this module.
pub const MODULE_NAME: &str = "citymodel-citygml";

const GML_NS: &str = "http://www.opengis.net/gml";
const BLDG_NS: &str = "http://www.opengis.net/citygml/building/2.0";
const DEM_NS: &str = "http://www.opengis.net/citygml/relief/2.0";
const APP_NS: &str = "http://www.opengis.net/citygml/appearance/2.0";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// Exposes the data-contract version consumed by parser output.
#[must_use]
pub fn contract_schema_version() -> &'static str {
    citymodel_core::CURRENT_CONTRACT_VERSION.schema_version
}

/// Bounds enforced while processing untrusted input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputLimits {
    /// Maximum nesting depth of XML elements.
    pub max_xml_depth: usize,
    /// Maximum UTF-8 text bytes collected for one coordinate element.
    pub max_coordinate_text_bytes: usize,
    /// Maximum values in one coordinate sequence.
    pub max_coordinate_values: usize,
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_xml_depth: 128,
            max_coordinate_text_bytes: 4 * 1024 * 1024,
            max_coordinate_values: 1_000_000,
        }
    }
}

/// A file selected for parsing, with its stable source digest.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputFile {
    pub path: PathBuf,
    pub sha256: String,
}

/// A namespace-qualified name retained in parser diagnostics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QualifiedName {
    pub namespace_uri: Option<String>,
    pub local_name: String,
}

/// The axis order declared by the source CRS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AxisOrder {
    EastNorthUp,
    NorthEastUp,
    Unknown,
}

/// A coordinate sequence emitted without constructing a document tree.
#[derive(Clone, Debug, PartialEq)]
pub struct CoordinateSequence {
    pub values: Vec<f64>,
    pub dimension: Option<u8>,
    pub srs_name: Option<String>,
    pub axis_order: AxisOrder,
    pub is_linear_ring: bool,
    /// The `CityGML` `lodN*` ancestor that owns this coordinate sequence, when present.
    pub lod: Option<u8>,
    /// `gml:id` of the containing terrain ring or surface, when available.
    pub surface_id: Option<String>,
}

/// A typed scalar value declared on a `bldg:Building`.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    Code(String),
    Real(f64),
}

/// A namespace-qualified Building attribute emitted from the source document.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildingAttribute {
    pub namespace_uri: String,
    pub attribute_path: String,
    pub attribute_key: String,
    pub value: AttributeValue,
    pub uom: Option<String>,
    pub code_space: Option<String>,
    pub nil_reason: Option<String>,
}

/// A parser event consumed by normalization and CRS stages.
#[derive(Clone, Debug, PartialEq)]
pub enum ParserEvent {
    StartFeature {
        gml_id: String,
        is_building_part: bool,
        feature_type: FeatureType,
    },
    EndFeature,
    Coordinates(CoordinateSequence),
    BuildingAttribute(BuildingAttribute),
    /// A texture declaration associated with a terrain surface or ring.
    TerrainTexture(TerrainTexture),
    /// Same-file `XLink`. `target_id` is populated for `href="#..."`.
    XLink {
        href: String,
        target_id: Option<String>,
        resolved: bool,
    },
}

/// `CityGML` feature kinds consumed by the converter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeatureType {
    Building,
    Terrain,
}

/// A local `app:ParameterizedTexture` reference.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainTexture {
    pub target_id: String,
    pub image_uri: String,
    /// UV pairs in the order declared by `app:textureCoordinates`.
    pub coordinates: Vec<(f64, f64)>,
}

/// A non-fatal issue associated with an input file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
}

/// Classification used by callers to decide strict or tolerant handling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    InvalidXml,
    ForbiddenDtd,
    LimitExceeded,
    MissingId,
    UnsupportedElement,
    InvalidCoordinate,
    InvalidAttribute,
    UnresolvedXLink,
    UnsafeArchivePath,
    Io,
    InvalidTexture,
}

/// Result of parsing one source file.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseReport {
    pub input: InputFile,
    pub events: Vec<ParserEvent>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Finds `CityGML` files under a single file or a PLATEAU dataset's supported modules.
///
/// # Errors
///
/// Returns an I/O diagnostic when the input cannot be read.
pub fn discover_input_files(path: impl AsRef<Path>) -> Result<Vec<InputFile>, Diagnostic> {
    let path = path.as_ref();
    let files = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        let mut files = Vec::new();
        for module in ["bldg", "dem"] {
            let root = path.join("udx").join(module);
            if root.is_dir() {
                files.extend(collect_citygml_files(&root)?);
            }
        }
        files.sort();
        files
    };

    files.into_iter().map(input_file).collect()
}

/// Parses one selected `CityGML` file as an event stream.
#[must_use]
#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
pub fn parse_file(input: InputFile, limits: InputLimits) -> ParseReport {
    let mut report = ParseReport {
        input: input.clone(),
        events: Vec::new(),
        diagnostics: Vec::new(),
    };
    let file = match File::open(&input.path) {
        Ok(file) => file,
        Err(error) => {
            report.diagnostics.push(io_diagnostic(error));
            return report;
        }
    };

    let mut reader = NsReader::from_reader(BufReader::new(HashingReader::new(file)));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut contexts: Vec<ElementContext> = Vec::new();
    let mut coordinate: Option<PendingCoordinates> = None;
    let mut attribute: Option<PendingAttribute> = None;
    let mut texture: Option<PendingTexture> = None;
    let mut texture_text: Option<PendingTextureText> = None;
    let mut feature_depths = BTreeSet::new();
    let mut known_gml_ids = BTreeSet::new();

    loop {
        let resolved = reader.read_resolved_event_into(&mut buffer);
        let (namespace, event) = match resolved {
            Ok(value) => value,
            Err(error) => {
                report.diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::InvalidXml,
                    message: error.to_string(),
                });
                break;
            }
        };
        match event {
            Event::DocType(_) => {
                report.diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::ForbiddenDtd,
                    message: "DTD and entity declarations are not accepted".to_owned(),
                });
                break;
            }
            Event::Start(start) => {
                let name = qualified_name(namespace, start.local_name().as_ref());
                if contexts.len() >= limits.max_xml_depth {
                    report.diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::LimitExceeded,
                        message: format!("XML depth exceeds {}", limits.max_xml_depth),
                    });
                    break;
                }
                let parent = contexts.last().cloned().unwrap_or_default();
                let context = element_context(&reader, &start, &parent, &name, contexts.len());
                if let Some(gml_id) = &context.gml_id {
                    known_gml_ids.insert(gml_id.clone());
                }
                handle_start(
                    &context,
                    &mut report,
                    &mut coordinate,
                    &mut attribute,
                    &mut texture,
                    &mut texture_text,
                    &mut feature_depths,
                );
                contexts.push(context);
            }
            Event::Empty(start) => {
                let mut name = qualified_name(namespace, start.local_name().as_ref());
                if name.namespace_uri.is_none() && declares_namespace(&start, BLDG_NS) {
                    name.namespace_uri = Some(BLDG_NS.to_owned());
                }
                let parent = contexts.last().cloned().unwrap_or_default();
                let context = element_context(&reader, &start, &parent, &name, contexts.len());
                if let Some(gml_id) = &context.gml_id {
                    known_gml_ids.insert(gml_id.clone());
                }
                handle_start(
                    &context,
                    &mut report,
                    &mut coordinate,
                    &mut attribute,
                    &mut texture,
                    &mut texture_text,
                    &mut feature_depths,
                );
                handle_end(
                    &context,
                    &mut report,
                    &mut coordinate,
                    &mut attribute,
                    &mut texture,
                    &mut texture_text,
                    &mut feature_depths,
                    limits,
                );
            }
            Event::Text(text) => {
                if let Some(pending) = &mut coordinate {
                    let value = text.as_ref();
                    if pending.text.len() + value.len() <= limits.max_coordinate_text_bytes {
                        pending.text.push_str(value);
                        pending.text.push(' ');
                    } else {
                        pending.rejected = true;
                        report.diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::LimitExceeded,
                            message: "coordinate text exceeds configured limit".to_owned(),
                        });
                    }
                }
                if let Some(pending) = &mut attribute {
                    pending.text.push_str(text.as_ref());
                }
                append_texture_text(&mut texture, texture_text, text.as_ref());
            }
            Event::CData(text) => {
                if let Some(pending) = &mut coordinate {
                    let value = text.as_ref();
                    if pending.text.len() + value.len() <= limits.max_coordinate_text_bytes {
                        pending.text.push_str(value);
                        pending.text.push(' ');
                    } else {
                        pending.rejected = true;
                        report.diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::LimitExceeded,
                            message: "coordinate text exceeds configured limit".to_owned(),
                        });
                    }
                }
                if let Some(pending) = &mut attribute {
                    pending.text.push_str(text.as_ref());
                }
                append_texture_text(&mut texture, texture_text, text.as_ref());
            }
            Event::End(_) => {
                if let Some(context) = contexts.pop() {
                    handle_end(
                        &context,
                        &mut report,
                        &mut coordinate,
                        &mut attribute,
                        &mut texture,
                        &mut texture_text,
                        &mut feature_depths,
                        limits,
                    );
                }
            }
            Event::Eof => {
                if !contexts.is_empty() {
                    report.diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::InvalidXml,
                        message: "unexpected end of input before all XML elements closed"
                            .to_owned(),
                    });
                }
                break;
            }
            _ => {}
        }
        buffer.clear();
    }
    let hasher = reader.into_inner().into_inner().hasher;
    report.input.sha256 = hexadecimal_digest(hasher.finalize());
    resolve_same_file_xlinks(&mut report, &known_gml_ids);
    report
}

/// Rejects `ZIP` entry paths that could escape the dataset root.
///
/// # Errors
///
/// Returns an unsafe-path diagnostic when the path is absolute or contains `..`.
pub fn validate_archive_entry_path(path: impl AsRef<Path>) -> Result<(), Diagnostic> {
    let path = path.as_ref();
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Diagnostic {
            kind: DiagnosticKind::UnsafeArchivePath,
            message: format!("unsafe archive entry path: {}", path.display()),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
struct ElementContext {
    name: QualifiedName,
    srs_name: Option<String>,
    dimension: Option<u8>,
    inside_linear_ring: bool,
    lod: Option<u8>,
    depth: usize,
    xlink_href: Option<String>,
    texture_target: Option<String>,
    gml_id: Option<String>,
    uom: Option<String>,
    code_space: Option<String>,
    nil_reason: Option<String>,
    inside_building_feature: bool,
    inside_terrain_feature: bool,
    terrain_surface_id: Option<String>,
}

#[derive(Clone, Debug)]
struct PendingCoordinates {
    context: ElementContext,
    text: String,
    rejected: bool,
}

#[derive(Clone, Debug)]
struct PendingAttribute {
    context: ElementContext,
    text: String,
}

#[derive(Clone, Debug)]
struct PendingTexture {
    target_id: Option<String>,
    image_uri: Option<String>,
    coordinate_text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingTextureText {
    ImageUri,
    Coordinates,
}

struct HashingReader {
    inner: File,
    hasher: Sha256,
}

impl HashingReader {
    fn new(inner: File) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }
}

impl Read for HashingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

fn collect_citygml_files(root: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
    let mut files = Vec::new();
    let entries = fs::read_dir(root).map_err(io_diagnostic)?;
    for entry in entries {
        let path = entry.map_err(io_diagnostic)?.path();
        if path.is_dir() {
            files.extend(collect_citygml_files(&path)?);
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("gml" | "xml")
        ) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn input_file(path: PathBuf) -> Result<InputFile, Diagnostic> {
    let mut file = File::open(&path).map_err(io_diagnostic)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io_diagnostic)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(InputFile {
        path,
        sha256: hexadecimal_digest(hasher.finalize()),
    })
}

fn hexadecimal_digest(digest: impl IntoIterator<Item = u8>) -> String {
    let mut value = String::with_capacity(64);
    for byte in digest {
        write!(value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

#[allow(clippy::needless_pass_by_value)]
fn io_diagnostic(error: std::io::Error) -> Diagnostic {
    Diagnostic {
        kind: DiagnosticKind::Io,
        message: error.to_string(),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn qualified_name(namespace: ResolveResult<'_>, local: &str) -> QualifiedName {
    let namespace_uri = match namespace {
        ResolveResult::Bound(value) => Some(value.as_ref().to_owned()),
        ResolveResult::Unbound | ResolveResult::Unknown(_) => None,
    };
    QualifiedName {
        namespace_uri,
        local_name: local.to_owned(),
    }
}

fn element_context(
    reader: &NsReader<BufReader<HashingReader>>,
    start: &BytesStart<'_>,
    parent: &ElementContext,
    name: &QualifiedName,
    depth: usize,
) -> ElementContext {
    let mut context = ElementContext {
        name: name.clone(),
        srs_name: parent.srs_name.clone(),
        dimension: parent.dimension,
        inside_linear_ring: parent.inside_linear_ring || is_gml(name, "LinearRing"),
        inside_building_feature: parent.inside_building_feature
            || is_bldg(name, "Building")
            || is_bldg(name, "BuildingPart"),
        inside_terrain_feature: parent.inside_terrain_feature || is_dem(name, "ReliefFeature"),
        lod: lod_from_element_name(&name.local_name).or(parent.lod),
        depth,
        ..ElementContext::default()
    };
    for attribute in start.attributes().flatten() {
        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
        let value = match attribute.normalized_value(XmlVersion::Implicit1_0) {
            Ok(value) => value.into_owned(),
            Err(_) => continue,
        };
        match local.as_ref() {
            "id" if is_bound(&namespace, GML_NS) => context.gml_id = Some(value),
            "srsName"
                if is_bound(&namespace, GML_NS) || matches!(namespace, ResolveResult::Unbound) =>
            {
                context.srs_name = Some(value);
            }
            "srsDimension"
                if is_bound(&namespace, GML_NS) || matches!(namespace, ResolveResult::Unbound) =>
            {
                context.dimension = value.parse().ok();
            }
            "href" if is_bound(&namespace, XLINK_NS) => context.xlink_href = Some(value),
            "uri" if matches!(namespace, ResolveResult::Unbound) => {
                context.texture_target = Some(value);
            }
            "uom" if matches!(namespace, ResolveResult::Unbound) => context.uom = Some(value),
            "codeSpace" if matches!(namespace, ResolveResult::Unbound) => {
                context.code_space = Some(value);
            }
            "nilReason"
                if is_bound(&namespace, GML_NS) || matches!(namespace, ResolveResult::Unbound) =>
            {
                context.nil_reason = Some(value);
            }
            _ => {}
        }
    }
    if context.inside_terrain_feature
        && context.gml_id.is_some()
        && (is_gml(name, "LinearRing") || is_gml(name, "Polygon") || is_gml(name, "Triangle"))
    {
        context.terrain_surface_id = context.gml_id.clone();
    } else {
        context
            .terrain_surface_id
            .clone_from(&parent.terrain_surface_id);
    }
    context
}

fn handle_start(
    context: &ElementContext,
    report: &mut ParseReport,
    coordinate: &mut Option<PendingCoordinates>,
    attribute: &mut Option<PendingAttribute>,
    texture: &mut Option<PendingTexture>,
    texture_text: &mut Option<PendingTextureText>,
    feature_depths: &mut BTreeSet<usize>,
) {
    if is_bldg(&context.name, "Building")
        || is_bldg(&context.name, "BuildingPart")
        || is_dem(&context.name, "ReliefFeature")
    {
        if let Some(gml_id) = &context.gml_id {
            report.events.push(ParserEvent::StartFeature {
                gml_id: gml_id.clone(),
                is_building_part: is_bldg(&context.name, "BuildingPart"),
                feature_type: if is_dem(&context.name, "ReliefFeature") {
                    FeatureType::Terrain
                } else {
                    FeatureType::Building
                },
            });
            feature_depths.insert(context.depth);
        } else {
            report.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::MissingId,
                message: format!("{} has no gml:id", context.name.local_name),
            });
        }
    }
    if is_app(&context.name, "ParameterizedTexture") {
        *texture = Some(PendingTexture {
            target_id: None,
            image_uri: None,
            coordinate_text: String::new(),
        });
    }
    if texture.is_some() {
        if is_app(&context.name, "imageURI") {
            *texture_text = Some(PendingTextureText::ImageUri);
        } else if is_app(&context.name, "textureCoordinates") {
            *texture_text = Some(PendingTextureText::Coordinates);
        } else if is_app(&context.name, "target") {
            if let Some(href) = context
                .texture_target
                .as_ref()
                .or(context.xlink_href.as_ref())
            {
                if let Some(pending) = texture {
                    pending.target_id = href.strip_prefix('#').map(str::to_owned);
                }
            }
        }
    }
    if let Some(href) = &context.xlink_href {
        report.events.push(ParserEvent::XLink {
            href: href.clone(),
            target_id: href.strip_prefix('#').map(str::to_owned),
            resolved: false,
        });
    }
    if is_gml(&context.name, "pos") || is_gml(&context.name, "posList") {
        *coordinate = Some(PendingCoordinates {
            context: context.clone(),
            text: String::new(),
            rejected: false,
        });
    }
    if context.inside_building_feature
        && (is_bldg(&context.name, "usage") || is_bldg(&context.name, "measuredHeight"))
    {
        *attribute = Some(PendingAttribute {
            context: context.clone(),
            text: String::new(),
        });
    }
}

fn resolve_same_file_xlinks(report: &mut ParseReport, known_gml_ids: &BTreeSet<String>) {
    for event in &mut report.events {
        if let ParserEvent::XLink {
            target_id: Some(target_id),
            resolved,
            ..
        } = event
        {
            *resolved = known_gml_ids.contains(target_id);
            if !*resolved {
                report.diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::UnresolvedXLink,
                    message: format!("same-file XLink target not found: #{target_id}"),
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_end(
    context: &ElementContext,
    report: &mut ParseReport,
    coordinate: &mut Option<PendingCoordinates>,
    attribute: &mut Option<PendingAttribute>,
    texture: &mut Option<PendingTexture>,
    texture_text: &mut Option<PendingTextureText>,
    feature_depths: &mut BTreeSet<usize>,
    limits: InputLimits,
) {
    if is_gml(&context.name, "pos") || is_gml(&context.name, "posList") {
        if let Some(pending) = coordinate.take() {
            emit_coordinates(pending, report, limits);
        }
    }
    if is_bldg(&context.name, "usage") || is_bldg(&context.name, "measuredHeight") {
        if let Some(pending) = attribute.take() {
            emit_attribute(pending, report);
        }
    }
    if is_app(&context.name, "imageURI") || is_app(&context.name, "textureCoordinates") {
        *texture_text = None;
    }
    if is_app(&context.name, "ParameterizedTexture") {
        if let Some(pending) = texture.take() {
            emit_terrain_texture(pending, report);
        }
        *texture_text = None;
    }
    if feature_depths.remove(&context.depth) {
        report.events.push(ParserEvent::EndFeature);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn emit_coordinates(pending: PendingCoordinates, report: &mut ParseReport, limits: InputLimits) {
    if pending.rejected {
        return;
    }
    let mut values = Vec::new();
    for token in pending.text.split_whitespace() {
        if values.len() == limits.max_coordinate_values {
            report.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::LimitExceeded,
                message: "coordinate value count exceeds configured limit".to_owned(),
            });
            return;
        }
        if let Ok(value) = token.parse() {
            values.push(value);
        } else {
            report.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::InvalidCoordinate,
                message: "coordinate sequence contains a non-numeric value".to_owned(),
            });
            return;
        }
    }
    report
        .events
        .push(ParserEvent::Coordinates(CoordinateSequence {
            values,
            dimension: pending.context.dimension,
            srs_name: pending.context.srs_name.clone(),
            axis_order: axis_order(pending.context.srs_name.as_deref()),
            is_linear_ring: pending.context.inside_linear_ring,
            lod: pending.context.lod,
            surface_id: pending.context.terrain_surface_id.clone(),
        }));
}

fn append_texture_text(
    texture: &mut Option<PendingTexture>,
    kind: Option<PendingTextureText>,
    text: &str,
) {
    let (Some(texture), Some(kind)) = (texture, kind) else {
        return;
    };
    match kind {
        PendingTextureText::ImageUri => texture
            .image_uri
            .get_or_insert_with(String::new)
            .push_str(text),
        PendingTextureText::Coordinates => {
            texture.coordinate_text.push_str(text);
            texture.coordinate_text.push(' ');
        }
    }
}

fn emit_terrain_texture(pending: PendingTexture, report: &mut ParseReport) {
    let Some(target_id) = pending.target_id else {
        report.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::InvalidTexture,
            message: "app:ParameterizedTexture has no same-file target URI".to_owned(),
        });
        return;
    };
    let Some(image_uri) = pending.image_uri.map(|value| value.trim().to_owned()) else {
        report.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::InvalidTexture,
            message: "app:ParameterizedTexture has no imageURI".to_owned(),
        });
        return;
    };
    let values = pending
        .coordinate_text
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>();
    let Ok(values) = values else {
        report.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::InvalidTexture,
            message: "texture coordinates contain a non-numeric value".to_owned(),
        });
        return;
    };
    if values.is_empty() || values.len() % 2 != 0 || values.iter().any(|value| !value.is_finite()) {
        report.diagnostics.push(Diagnostic {
            kind: DiagnosticKind::InvalidTexture,
            message: "texture coordinates must be finite UV pairs".to_owned(),
        });
        return;
    }
    report
        .events
        .push(ParserEvent::TerrainTexture(TerrainTexture {
            target_id,
            image_uri,
            coordinates: values
                .chunks_exact(2)
                .map(|pair| (pair[0], pair[1]))
                .collect(),
        }));
}

fn emit_attribute(pending: PendingAttribute, report: &mut ParseReport) {
    let value = pending.text.trim();
    let namespace_uri = pending
        .context
        .name
        .namespace_uri
        .clone()
        .unwrap_or_else(|| BLDG_NS.to_owned());
    let attribute_key = pending.context.name.local_name.clone();
    let value = match attribute_key.as_str() {
        "usage" if value.is_empty() => {
            report.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::InvalidAttribute,
                message: "bldg:usage must contain a code value".to_owned(),
            });
            return;
        }
        "usage" => AttributeValue::Code(value.to_owned()),
        "measuredHeight" => match value.parse::<f64>() {
            Ok(value) if value.is_finite() => AttributeValue::Real(value),
            _ => {
                report.diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::InvalidAttribute,
                    message: format!("bldg:measuredHeight is not a finite number: {value}"),
                });
                return;
            }
        },
        _ => return,
    };
    report
        .events
        .push(ParserEvent::BuildingAttribute(BuildingAttribute {
            namespace_uri,
            attribute_path: attribute_key.clone(),
            attribute_key,
            value,
            uom: pending.context.uom,
            code_space: pending.context.code_space,
            nil_reason: pending.context.nil_reason,
        }));
}

fn lod_from_element_name(name: &str) -> Option<u8> {
    let suffix = name.strip_prefix("lod")?;
    suffix
        .chars()
        .next()?
        .to_digit(10)
        .and_then(|value| u8::try_from(value).ok())
}

fn axis_order(srs_name: Option<&str>) -> AxisOrder {
    match srs_name {
        Some(value) if value.contains("EPSG:6697") || value.contains("EPSG::6697") => {
            AxisOrder::NorthEastUp
        }
        Some(value) if value.contains("EPSG:6668") || value.contains("EPSG::6668") => {
            AxisOrder::NorthEastUp
        }
        Some(_) | None => AxisOrder::Unknown,
    }
}

fn is_bound(namespace: &ResolveResult<'_>, expected: &str) -> bool {
    matches!(namespace, ResolveResult::Bound(value) if value.as_ref() == expected)
}
fn is_gml(name: &QualifiedName, local: &str) -> bool {
    name.namespace_uri.as_deref() == Some("http://www.opengis.net/gml") && name.local_name == local
}
fn is_bldg(name: &QualifiedName, local: &str) -> bool {
    name.namespace_uri.as_deref() == Some("http://www.opengis.net/citygml/building/2.0")
        && name.local_name == local
}
fn is_dem(name: &QualifiedName, local: &str) -> bool {
    name.namespace_uri.as_deref() == Some(DEM_NS) && name.local_name == local
}
fn is_app(name: &QualifiedName, local: &str) -> bool {
    name.namespace_uri.as_deref() == Some(APP_NS) && name.local_name == local
}

fn declares_namespace(start: &BytesStart<'_>, expected: &str) -> bool {
    start.attributes().flatten().any(|attribute| {
        attribute.key.as_ref().starts_with("xmlns")
            && attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .is_ok_and(|value| value == expected)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);

    fn sample_file(contents: &str) -> InputFile {
        let path = std::env::temp_dir().join(format!(
            "citygml-input-{}-{}.gml",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = File::create(&path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        input_file(path).unwrap()
    }

    #[test]
    fn streams_namespace_qualified_citygml_events() {
        let input = sample_file(
            r##"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:shape="http://www.opengis.net/citygml/building/2.0" xmlns:geo="http://www.opengis.net/gml" xmlns:ref="http://www.w3.org/1999/xlink"><shape:Building geo:id="b-1"><geo:LinearRing srsName="urn:ogc:def:crs:EPSG::6697" srsDimension="3"><geo:posList>35 139 10 35.1 139.1 11</geo:posList></geo:LinearRing><shape:consistsOfBuildingPart ref:href="#b-1"/></shape:Building></core:CityModel>"##,
        );
        let report = parse_file(input, InputLimits::default());
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(
            matches!(report.events[0], ParserEvent::StartFeature { ref gml_id, .. } if gml_id == "b-1")
        );
        assert!(
            matches!(&report.events[1], ParserEvent::Coordinates(sequence) if sequence.is_linear_ring && sequence.dimension == Some(3) && sequence.axis_order == AxisOrder::NorthEastUp),
            "{:?}",
            report.events
        );
        assert!(
            matches!(&report.events[2], ParserEvent::XLink { target_id: Some(target), resolved: true, .. } if target == "b-1")
        );
    }

    #[test]
    fn rejects_dtd_without_panicking() {
        let report = parse_file(
            sample_file(
                "<!DOCTYPE doc [<!ENTITY xxe SYSTEM 'file:///etc/passwd'>]><doc>&xxe;</doc>",
            ),
            InputLimits::default(),
        );
        assert_eq!(report.diagnostics[0].kind, DiagnosticKind::ForbiddenDtd);
    }

    #[test]
    fn reports_missing_id_and_invalid_xml() {
        let no_id = parse_file(
            sample_file(r#"<b:Building xmlns:b="http://www.opengis.net/citygml/building/2.0"/>"#),
            InputLimits::default(),
        );
        assert_eq!(
            no_id.diagnostics[0].kind,
            DiagnosticKind::MissingId,
            "{no_id:?}"
        );
        let invalid = parse_file(sample_file("<broken>"), InputLimits::default());
        assert_eq!(invalid.diagnostics[0].kind, DiagnosticKind::InvalidXml);
    }

    #[test]
    fn emits_typed_building_attributes_with_units_and_code_spaces() {
        let report = parse_file(
            sample_file(
                r#"<b:Building xmlns:b="http://www.opengis.net/citygml/building/2.0" xmlns:g="http://www.opengis.net/gml" g:id="b-1"><b:usage codeSpace="urn:usage">residential</b:usage><b:measuredHeight uom="m">12.5</b:measuredHeight></b:Building>"#,
            ),
            InputLimits::default(),
        );
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
        assert!(matches!(
            &report.events[1],
            ParserEvent::BuildingAttribute(BuildingAttribute {
                namespace_uri,
                attribute_key,
                value: AttributeValue::Code(value),
                code_space: Some(code_space),
                ..
            }) if namespace_uri == BLDG_NS && attribute_key == "usage" && value == "residential" && code_space == "urn:usage"
        ));
        assert!(matches!(
            &report.events[2],
            ParserEvent::BuildingAttribute(BuildingAttribute {
                attribute_key,
                value: AttributeValue::Real(value),
                uom: Some(uom),
                ..
            }) if attribute_key == "measuredHeight" && (*value - 12.5).abs() < f64::EPSILON && uom == "m"
        ));
    }

    #[test]
    fn reports_invalid_measured_height() {
        let report = parse_file(
            sample_file(
                r#"<b:Building xmlns:b="http://www.opengis.net/citygml/building/2.0" xmlns:g="http://www.opengis.net/gml" g:id="b-1"><b:measuredHeight uom="m">high</b:measuredHeight></b:Building>"#,
            ),
            InputLimits::default(),
        );
        assert_eq!(report.diagnostics[0].kind, DiagnosticKind::InvalidAttribute);
    }

    #[test]
    fn emits_terrain_surface_identity_and_parameterized_texture() {
        let input = sample_file(
            r##"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:dem="http://www.opengis.net/citygml/relief/2.0" xmlns:app="http://www.opengis.net/citygml/appearance/2.0" xmlns:gml="http://www.opengis.net/gml"><dem:ReliefFeature gml:id="terrain-1"><gml:Triangle gml:id="surface-1"><gml:LinearRing gml:id="ring-1" srsName="urn:ogc:def:crs:EPSG::6697"><gml:posList>35 139 0 35 139.1 0 35.1 139 0</gml:posList></gml:LinearRing></gml:Triangle></dem:ReliefFeature><app:ParameterizedTexture><app:imageURI>terrain.png</app:imageURI><app:target uri="#ring-1"><app:TexCoordList><app:textureCoordinates>0 0 1 0 0 1</app:textureCoordinates></app:TexCoordList></app:target></app:ParameterizedTexture></core:CityModel>"##,
        );
        let report = parse_file(input, InputLimits::default());
        assert!(matches!(
            report.events.first(),
            Some(ParserEvent::StartFeature {
                feature_type: FeatureType::Terrain,
                ..
            })
        ));
        assert!(report.events.iter().any(|event| matches!(event, ParserEvent::Coordinates(sequence) if sequence.surface_id.as_deref() == Some("ring-1"))));
        assert!(report.events.iter().any(|event| matches!(event, ParserEvent::TerrainTexture(texture) if texture.target_id == "ring-1" && texture.coordinates.len() == 3)));
    }

    #[test]
    fn rejects_unsafe_archive_paths() {
        assert!(validate_archive_entry_path("udx/bldg/valid.gml").is_ok());
        assert_eq!(
            validate_archive_entry_path("../escape.gml")
                .unwrap_err()
                .kind,
            DiagnosticKind::UnsafeArchivePath
        );
    }
}
