using System;
using System.Collections.Generic;
using CityModel.Georeference;
using UnityEngine;

namespace CityModel.Picking
{
    public readonly struct BuildingPickResult { public readonly string BuildingId; public readonly string BuildingPartId; public readonly string TileId; public readonly ushort FeatureId; public readonly Vector3 UnityPosition; public readonly ProjectedCoordinate ProjectedPosition; public readonly Vector3 Normal; public readonly float Distance; public BuildingPickResult(string buildingId, string partId, string tileId, ushort featureId, Vector3 position, ProjectedCoordinate projected, Vector3 normal, float distance) { BuildingId = buildingId; BuildingPartId = partId; TileId = tileId; FeatureId = featureId; UnityPosition = position; ProjectedPosition = projected; Normal = normal; Distance = distance; } }

    /// <summary>Maps collider triangle indices explicitly to tile-local Feature IDs.</summary>
    public sealed class BuildingPicker
    {
        private readonly GeoReference _geoReference;
        public BuildingPicker(GeoReference geoReference) { _geoReference = geoReference; }
        public bool TryPick(Ray ray, float maxDistance, IReadOnlyList<string> buildingIds, IReadOnlyList<string> buildingPartIds, string tileId, ushort[] triangleFeatureIds, out BuildingPickResult result)
        {
            if (!Physics.Raycast(ray, out var hit, maxDistance) || hit.collider is not MeshCollider || hit.triangleIndex < 0 || hit.triangleIndex >= triangleFeatureIds.Length) { result = default; return false; }
            var featureId = triangleFeatureIds[hit.triangleIndex];
            if (featureId >= buildingIds.Count) { result = default; return false; }
            var partId = featureId < buildingPartIds.Count ? buildingPartIds[featureId] : null;
            result = new BuildingPickResult(buildingIds[featureId], partId, tileId, featureId, hit.point, _geoReference.UnityToProjected(hit.point), hit.normal, hit.distance);
            return true;
        }
    }
}
