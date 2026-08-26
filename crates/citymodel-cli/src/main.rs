//! Placeholder command-line entry point for the `CityGML` converter.

fn main() {
    let modules = [
        citymodel_citygml::MODULE_NAME,
        citymodel_coordinate::MODULE_NAME,
        citymodel_geometry::MODULE_NAME,
        citymodel_tiling::MODULE_NAME,
        citymodel_gltf::MODULE_NAME,
        citymodel_spatialite::MODULE_NAME,
        citymodel_validation::MODULE_NAME,
    ];

    println!(
        "citymodel converter bootstrap (schema v{}): {}",
        citymodel_citygml::contract_schema_version(),
        modules.join(", ")
    );
}
