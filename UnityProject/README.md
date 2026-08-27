# CityGMLLite Unity project

This is the runnable Unity 6.5.9f1 project for the local
`com.azarashin.citymodel` package. It references the package by relative path,
so package and runtime changes are immediately visible without publishing it.

## Run

1. Generate a dataset from a PLATEAU LOD1 CityGML file:

   ```powershell
   cargo run -p citymodel-cli -- convert "C:\CityGMLLiteData\input\building.gml" --output "C:\CityGMLLiteData\output\building" --tolerant
   ```

2. Open `UnityProject/` directly with Unity 6.5.9f1.
3. Open `Assets/QuickStart.unity`.
4. Set **Dataset Root** on **CityModel Quick Start** to the generated output
   directory containing `dataset.manifest.json`.
5. Enter Play mode. The Console reports how many tiles were rendered.

The first editor launch creates `Library/`, generated project settings, and the
package lock file. These generated files are ignored by Git.
