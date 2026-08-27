# Converter CLI guide

Use `citymodel inspect <CityGML file or dataset directory>` to inspect an input boundary.

Use `citymodel convert <input> --output <directory>` for strict conversion, or add
`--tolerant` to retain diagnostics instead of stopping at the first recoverable issue.
The converter writes to a temporary sibling directory and renames it only after a
successful write, preserving an existing valid output directory on failure.

The executable is built for Windows x64 with `cargo build --release -p citymodel-cli`.
