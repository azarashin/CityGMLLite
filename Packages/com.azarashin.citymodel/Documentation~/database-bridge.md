# Windows x64 database bridge

`BuildingDatabase` accepts a package-owned `IReadOnlyBuildingStore` implementation. The production Windows x64 bridge must open the generated `.sqlite` artifact in read-only mode, verify schemaVersion and generationId before queries, and expose only parameterized operations for building and attribute lookup. It must not expose arbitrary SQL, arbitrary database paths, or extension loading to Unity callers.
