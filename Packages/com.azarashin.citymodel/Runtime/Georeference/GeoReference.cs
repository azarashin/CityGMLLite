using System;
using UnityEngine;

namespace CityModel.Georeference
{
    [Serializable]
    public struct ProjectedCoordinate { public double East; public double North; public double Up; }

    /// <summary>Converts Working CRS ENU coordinates using the GLB X/East, Y/Up, Z/-North rule.</summary>
    public sealed class GeoReference
    {
        public GeoReference(ProjectedCoordinate datasetOrigin, ProjectedCoordinate sceneOrigin) { DatasetOrigin = datasetOrigin; SceneOrigin = sceneOrigin; }
        public ProjectedCoordinate DatasetOrigin { get; }
        public ProjectedCoordinate SceneOrigin { get; private set; }
        public void SetSceneOrigin(ProjectedCoordinate value) { SceneOrigin = value; }
        public Vector3 ProjectedToUnity(ProjectedCoordinate point) => new((float)(point.East - SceneOrigin.East), (float)(point.Up - SceneOrigin.Up), (float)(SceneOrigin.North - point.North));
        public ProjectedCoordinate UnityToProjected(Vector3 point) => new() { East = SceneOrigin.East + point.x, North = SceneOrigin.North - point.z, Up = SceneOrigin.Up + point.y };
    }
}
