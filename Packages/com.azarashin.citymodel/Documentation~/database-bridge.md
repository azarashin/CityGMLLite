# Windows x64 database bridge

`BuildingDatabase` accepts a package-owned `IReadOnlyBuildingStore` implementation. The production Windows x64 bridge opens the generated `.sqlite` artifact in read-only mode, verifies schemaVersion and generationId before queries, and exposes only parameterized operations. `FindBuildingAsync` and `FindAttributesAsync` remain the building-compatible API; `FindFeatureAsync` and `FindFeatureAttributesAsync` query the v2 common `features` and `feature_attributes` tables.

The bridge must not expose arbitrary SQL, arbitrary database paths, or extension loading to Unity callers. A `FeaturePicker` result contains the type, persistent feature ID, and tile-local feature ID. A type disabled by **Load On Startup** has no decoded mesh or collider and therefore cannot be picked. A loaded type which is initially hidden has its `MeshRenderer` disabled but retains its collider, so it remains pickable unless a future interaction policy disables that collider explicitly.
