use std::path::PathBuf;

use citymodel_citygml::{AxisOrder, InputLimits, ParserEvent, discover_input_files, parse_file};

#[test]
fn small_plateau_fixture_emits_stable_building_and_coordinates() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plateau-lod1-small.gml");
    let input = discover_input_files(path).unwrap().pop().unwrap();
    let report = parse_file(input, InputLimits::default());
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert!(
        matches!(report.events.first(), Some(ParserEvent::StartFeature { gml_id, .. }) if gml_id == "sample-building-1")
    );
    assert!(report.events.iter().any(|event| matches!(event, ParserEvent::Coordinates(sequence) if sequence.values.len() == 12 && sequence.axis_order == AxisOrder::NorthEastUp)));
}
