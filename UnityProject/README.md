# CityGMLLite Unity project

This is the runnable Unity 6.5.9f1 project for the local
`com.azarashin.citymodel` package. It references the package by relative path,
so package and runtime changes are immediately visible without publishing it.

## Run

1. Open `UnityProject/` directly with Unity 6.5.9f1.
2. Open `Assets/QuickStart.unity`.
3. Set **Dataset Root** on **CityModel Quick Start** to a converter output
   directory containing `dataset.manifest.json`.
4. Enter Play mode and inspect the Console.

The first editor launch creates `Library/`, generated project settings, and the
package lock file. These generated files are ignored by Git.
