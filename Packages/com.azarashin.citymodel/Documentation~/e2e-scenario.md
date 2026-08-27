# MVP E2E scenario

1. Convert a PLATEAU CityGML dataset with the `citymodel convert` command.
2. Open the output using `CityModelDatasetFacade.OpenAsync`.
3. Load a GLB tile, then validate Feature ID, color, CPU picking, building attribute lookup, and spatial lookup.
4. Close the facade twice and confirm the operations are idempotent.

The sample scene and real PLATEAU conversion data are release-gate artifacts; they are not committed yet because no licensed reference dataset has been provided to the repository.
