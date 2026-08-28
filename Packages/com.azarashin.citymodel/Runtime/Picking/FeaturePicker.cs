using System;
using System.Collections.Generic;
using CityModel.Georeference;
using CityModel.Loading;
using UnityEngine;

namespace CityModel.Picking
{
    /// <summary>Result of picking a loaded, type-specific tile mesh.</summary>
    public readonly struct FeaturePickResult
    {
        public readonly string FeatureType;
        public readonly string FeatureId;
        public readonly ushort LocalFeatureId;
        public readonly string TileId;
        public readonly Vector3 UnityPosition;
        public readonly ProjectedCoordinate ProjectedPosition;
        public readonly Vector3 Normal;
        public readonly float Distance;

        public FeaturePickResult(string featureType, string featureId, ushort localFeatureId, string tileId, Vector3 position, ProjectedCoordinate projected, Vector3 normal, float distance)
        {
            FeatureType = featureType;
            FeatureId = featureId;
            LocalFeatureId = localFeatureId;
            TileId = tileId;
            UnityPosition = position;
            ProjectedPosition = projected;
            Normal = normal;
            Distance = distance;
        }
    }

    /// <summary>
    /// Maps collider triangle indices to generic tile-local feature IDs. Only loaded
    /// types have colliders and can therefore be picked. Hidden loaded types retain
    /// their colliders, so their pickability is independent of renderer visibility.
    /// </summary>
    public sealed class FeaturePicker
    {
        private readonly GeoReference _geoReference;

        public FeaturePicker(GeoReference geoReference) { _geoReference = geoReference ?? throw new ArgumentNullException(nameof(geoReference)); }

        public bool TryPick(Ray ray, float maxDistance, IReadOnlyList<GenericTileFeature> features, string tileId, ushort[] triangleFeatureIds, out FeaturePickResult result)
        {
            if (!Physics.Raycast(ray, out var hit, maxDistance) || hit.collider is not MeshCollider || hit.triangleIndex < 0 || triangleFeatureIds == null || hit.triangleIndex >= triangleFeatureIds.Length)
            {
                result = default;
                return false;
            }
            if (!TryResolveFeature(features, triangleFeatureIds[hit.triangleIndex], out var feature))
            {
                result = default;
                return false;
            }
            result = new FeaturePickResult(feature.featureType, feature.featureId, checked((ushort)feature.localFeatureId), tileId, hit.point, _geoReference.UnityToProjected(hit.point), hit.normal, hit.distance);
            return true;
        }

        /// <summary>Resolves a GLB tile-local feature ID without physics, for UI and test callers.</summary>
        public static bool TryResolveFeature(IReadOnlyList<GenericTileFeature> features, ushort localFeatureId, out GenericTileFeature feature)
        {
            if (features != null)
            {
                for (var index = 0; index < features.Count; index++)
                {
                    var candidate = features[index];
                    if (candidate != null && candidate.localFeatureId == localFeatureId && !string.IsNullOrWhiteSpace(candidate.featureId) && !string.IsNullOrWhiteSpace(candidate.featureType))
                    {
                        feature = candidate;
                        return true;
                    }
                }
            }
            feature = null;
            return false;
        }
    }
}
