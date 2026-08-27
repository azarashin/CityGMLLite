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
}

/// A parser event consumed by normalization and CRS stages.
#[derive(Clone, Debug, PartialEq)]
pub enum ParserEvent {
    StartFeature {
        gml_id: String,
        is_building_part: bool,
    },
    EndFeature,
    Coordinates(CoordinateSequence),
    /// Same-file `XLink`. `target_id` is populated for `href="#..."`.
    XLink {
        href: String,
        target_id: Option<String>,
        resolved: bool,
    },
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
    UnresolvedXLink,
    UnsafeArchivePath,
    Io,
}

/// Result of parsing one source file.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseReport {
    pub input: InputFile,
    pub events: Vec<ParserEvent>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Finds `CityGML` files under a single file or a PLATEAU dataset's `udx/bldg` directory.
///
/// # Errors
///
/// Returns an I/O diagnostic when the input cannot be read.
pub fn discover_input_files(path: impl AsRef<Path>) -> Result<Vec<InputFile>, Diagnostic> {
    let path = path.as_ref();
    let files = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        let building_root = path.join("udx").join("bldg");
        collect_citygml_files(&building_root)?
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
                handle_start(&context, &mut report, &mut coordinate, &mut feature_depths);
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
                handle_start(&context, &mut report, &mut coordinate, &mut feature_depths);
                handle_end(
                    &context,
                    &mut report,
                    &mut coordinate,
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
            }
            Event::End(_) => {
                if let Some(context) = contexts.pop() {
                    handle_end(
                        &context,
                        &mut report,
                        &mut coordinate,
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
    depth: usize,
    xlink_href: Option<String>,
    gml_id: Option<String>,
}

#[derive(Clone, Debug)]
struct PendingCoordinates {
    context: ElementContext,
    text: String,
    rejected: bool,
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
            _ => {}
        }
    }
    context
}

fn handle_start(
    context: &ElementContext,
    report: &mut ParseReport,
    coordinate: &mut Option<PendingCoordinates>,
    feature_depths: &mut BTreeSet<usize>,
) {
    if is_bldg(&context.name, "Building") || is_bldg(&context.name, "BuildingPart") {
        if let Some(gml_id) = &context.gml_id {
            report.events.push(ParserEvent::StartFeature {
                gml_id: gml_id.clone(),
                is_building_part: is_bldg(&context.name, "BuildingPart"),
            });
            feature_depths.insert(context.depth);
        } else {
            report.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::MissingId,
                message: format!("{} has no gml:id", context.name.local_name),
            });
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

fn handle_end(
    context: &ElementContext,
    report: &mut ParseReport,
    coordinate: &mut Option<PendingCoordinates>,
    feature_depths: &mut BTreeSet<usize>,
    limits: InputLimits,
) {
    if is_gml(&context.name, "pos") || is_gml(&context.name, "posList") {
        if let Some(pending) = coordinate.take() {
            emit_coordinates(pending, report, limits);
        }
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
        }));
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
