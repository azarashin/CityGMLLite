# Converter CLI guide

Use `citymodel inspect <CityGML file or dataset directory>` to inspect an input boundary.

Use `citymodel convert <input> --output <directory>` for strict conversion, or add
`--tolerant` to retain diagnostics instead of stopping at the first recoverable issue.
The converter writes to a temporary sibling directory and renames it only after a
successful write, preserving an existing valid output directory on failure.

Use `--max-lod <0|1|2>` to set the highest source LOD eligible for conversion.
It defaults to `1`. For each building, the converter selects the highest available
LOD at or below that limit; for example, `--max-lod 2` selects LOD2 when present,
then falls back to LOD1 or LOD0. The selected level is recorded in
`citymodel.sqlite` as `buildings.lod_used`. A building with no eligible linear-ring
geometry fails strict conversion, or is recorded in `conversion_issues` when using
`--tolerant`. The manifest's schema-compatible `modelProfile.lod` records this
requested upper bound; consult each building's `lod_used` for the actual selection.

```powershell
cargo run -p citymodel-cli -- convert .\rawdata\city.gml --output .\output\city --max-lod 2 --tolerant
```

`citymodel inspect` reports the input's `lod0Rings`, `lod1Rings`, and `lod2Rings`
so the requested maximum can be chosen before conversion.

The executable is built for Windows x64 with `cargo build --release -p citymodel-cli`.
