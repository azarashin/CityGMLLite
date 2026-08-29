# CityModel Runtime package

This UPM package provides the Unity runtime boundary for CityGMLLite datasets.

## Local installation

In Unity Package Manager, choose **Add package from disk** and select this
directory's `package.json` file.

## Quick Start

1. Build the converter and prepare a dataset directory containing
   `dataset.manifest.json`.
2. In Package Manager, import **CityModel Runtime > Samples > Quick Start**.
3. Open `Assets/Samples/CityModel Runtime/0.1.0/Quick Start/QuickStart.unity`.
4. Select **CityModel Quick Start**, set **Dataset Root** to the converter output
   directory, then enter Play mode.
5. Under **Feature Type Startup Settings**, add one entry for each desired
   `featureType` (for example `building`, `terrain`, or `water`). Select
   **Load On Startup** to read and instantiate that type, and select
   **Initially Visible** to render it immediately. When loading is disabled,
   the type's metadata, GLB, textures, and attributes are not opened.
6. Choose **Attribute Color Mode** for loaded buildings: **Usage** applies
   stable categorical colours, while **Measured Height** applies a blue-to-red
   gradient over the dataset. Missing attributes are rendered gray.
7. Enter Play mode. Loaded-but-hidden types retain their decoded meshes and can
   be shown later without re-decoding by calling
   `SetFeatureTypeVisible(featureType, true)`. The sample validates only the
   selected content artifacts and opens the manifest-declared SQLite database
   only when buildings are selected.

`Dataset Root` must point at the converter output directory itself (the directory
that contains `dataset.manifest.json` and `citymodel.sqlite`), not at an
individual `.gml`, `.glb`, or `tiles` directory. Attribute colouring falls back
to the default colour on unsupported platforms or when the database cannot be
opened; validation errors are reported in the Console.
