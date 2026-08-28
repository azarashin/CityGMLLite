# CityModel Runtime package

This UPM package provides the Unity runtime boundary for CityGMLLite datasets.
It requires the Universal Render Pipeline (URP). The bundled Unity project is
configured with the CityModel URP asset; use the same pipeline when importing
the package into another project.

## Local installation

In Unity Package Manager, choose **Add package from disk** and select this
directory's `package.json` file.

## Quick Start

1. Build the converter and prepare a dataset directory containing
   `dataset.manifest.json`.
2. In Package Manager, import **CityModel Runtime > Samples > Quick Start**.
3. Open `Assets/Samples/CityModel Runtime/0.1.0/Quick Start/QuickStart.unity`.
   The project must use URP; the sample does not support the Built-in Render
   Pipeline.
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

Terrain content uses self-contained GLB files: `POSITION`, `NORMAL`,
`TEXCOORD_0`, `_FEATURE_ID_0`, an embedded PNG/JPEG image, and a glTF base-color
material. The runtime accepts only those embedded image forms, applies the
texture with Lambert lighting, and rejects external image URIs or oversized
image payloads. Terrain stays pickable through its generic `features.items`
mapping even when it starts hidden.

`Dataset Root` must point at the converter output directory itself (the directory
that contains `dataset.manifest.json` and `citymodel.sqlite`), not at an
individual `.gml`, `.glb`, or `tiles` directory. Attribute colouring falls back
to the default colour on unsupported platforms or when the database cannot be
opened; validation errors are reported in the Console.
