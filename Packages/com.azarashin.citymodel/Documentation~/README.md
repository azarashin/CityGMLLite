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
5. The Console reports the opened dataset ID and generation ID. Invalid schema,
   generation, or GLB hashes are reported as errors.

The sample only opens the manifest boundary. Loading rendered tiles, coloring,
CPU picking, and the native SQLite bridge require the corresponding runtime
features and real-data integration verification.
