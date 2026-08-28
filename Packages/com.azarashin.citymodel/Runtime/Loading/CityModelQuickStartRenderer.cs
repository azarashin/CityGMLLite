using System;
using System.Collections.Generic;
using System.Threading;
using CityModel.Coloring;
using CityModel.Database;
using CityModel.Loading;
using UnityEngine;

namespace CityModel.Samples
{
    /// <summary>
    /// Renders every inline tile of a generated dataset.
    /// Kept in Runtime so package samples and the bundled Unity project can both reference it.
    /// </summary>
    public sealed class CityModelQuickStartRenderer : MonoBehaviour
    {
        [Tooltip("Absolute path to a directory containing dataset.manifest.json.")]
        [SerializeField] private string datasetRoot;

        [Tooltip("Usage assigns stable categorical colors. Measured Height assigns a blue-to-red gradient across this dataset.")]
        [SerializeField] private BuildingAttributeColorMode attributeColorMode = BuildingAttributeColorMode.Usage;

        private CityModelDataset _dataset;
        private CancellationTokenSource _cancellation;
        private GameObject _tilesRoot;
        private Material _material;
        private BuildingColorService _colors;

        private async void Start()
        {
            if (string.IsNullOrWhiteSpace(datasetRoot))
            {
                Debug.Log("CityModel Quick Start: set Dataset Root in the Inspector, then press Play.");
                return;
            }

            _cancellation = new CancellationTokenSource();
            try
            {
                _dataset = await CityModelDataset.OpenAsync(datasetRoot, _cancellation.Token);
                _tilesRoot = new GameObject("CityModel Tiles");
                _tilesRoot.transform.SetParent(transform, false);
                _material = CreateMaterial();
                var loadedTiles = new List<LoadedTile>();
                foreach (var tile in _dataset.Manifest.tiles.items)
                {
                    _cancellation.Token.ThrowIfCancellationRequested();
                    loadedTiles.Add(await _dataset.LoadTileAsync(tile, _cancellation.Token));
                }

                _colors = new BuildingColorService(BuildingAttributeColorizer.MissingAttributeColor);
                await ApplyAttributeColorsAsync(loadedTiles, _cancellation.Token);
                foreach (var tile in loadedTiles) CreateTile(tile);

                Debug.Log($"CityModel Quick Start: rendered {_dataset.Manifest.tiles.items.Length} tile(s).", this);
            }
            catch (Exception exception)
            {
                Debug.LogException(exception, this);
            }
        }

        private async System.Threading.Tasks.Task ApplyAttributeColorsAsync(IReadOnlyList<LoadedTile> loadedTiles, CancellationToken cancellationToken)
        {
            var attributesByBuilding = new Dictionary<string, IReadOnlyList<BuildingAttribute>>();
            try
            {
                using (var database = await _dataset.OpenBuildingDatabaseAsync(cancellationToken))
                {
                    foreach (var loadedTile in loadedTiles)
                    {
                        var buildingIds = loadedTile.Metadata.features?.buildingIds;
                        if (buildingIds == null) continue;
                        foreach (var buildingId in buildingIds)
                        {
                            if (string.IsNullOrWhiteSpace(buildingId) || attributesByBuilding.ContainsKey(buildingId)) continue;
                            attributesByBuilding.Add(buildingId, await database.FindAttributesAsync(buildingId, cancellationToken));
                        }
                    }
                }
            }
            catch (PlatformNotSupportedException exception)
            {
                Debug.LogWarning("CityModel Quick Start: attribute coloring is unavailable on this platform; rendering default colors. " + exception.Message, this);
                return;
            }

            var minimumHeight = float.PositiveInfinity;
            var maximumHeight = float.NegativeInfinity;
            if (attributeColorMode == BuildingAttributeColorMode.MeasuredHeight)
            {
                foreach (var attributes in attributesByBuilding.Values)
                {
                    var height = BuildingAttributeColorizer.FindHeight(attributes);
                    if (!height.HasValue) continue;
                    minimumHeight = Mathf.Min(minimumHeight, height.Value);
                    maximumHeight = Mathf.Max(maximumHeight, height.Value);
                }
            }

            foreach (var pair in attributesByBuilding)
            {
                _colors.SetColor(pair.Key, BuildingAttributeColorizer.ColorFor(attributeColorMode, pair.Value, minimumHeight, maximumHeight));
            }
        }

        private void CreateTile(LoadedTile loadedTile)
        {
            var tileOrigin = loadedTile.Metadata.origin?.projected;
            var datasetOrigin = _dataset.Manifest.datasetOrigin?.projected;
            if (tileOrigin == null || datasetOrigin == null)
                throw new InvalidOperationException("Dataset or tile projected origin is missing.");

            var tile = new GameObject(loadedTile.Metadata.tileId);
            tile.transform.SetParent(_tilesRoot.transform, false);
            tile.transform.localPosition = new Vector3(
                (float)(tileOrigin.x - datasetOrigin.x),
                (float)(tileOrigin.z - datasetOrigin.z),
                (float)(datasetOrigin.y - tileOrigin.y));
            var decoded = CityModelGlbDecoder.DecodeWithFeatureIds(loadedTile.GlbBytes, loadedTile.Metadata.tileId);
            var buildingIds = loadedTile.Metadata.features?.buildingIds ?? Array.Empty<string>();
            ValidateFeatureIds(decoded.FeatureIds, buildingIds, loadedTile.Metadata.tileId);
            tile.AddComponent<MeshFilter>().sharedMesh = decoded.Mesh;
            var renderer = tile.AddComponent<MeshRenderer>();
            renderer.sharedMaterial = _material;
            _colors.RegisterTile(loadedTile.Metadata.tileId, buildingIds);
            _colors.ApplyToRenderer(loadedTile.Metadata.tileId, renderer);
        }

        private static void ValidateFeatureIds(IReadOnlyList<ushort> featureIds, IReadOnlyList<string> buildingIds, string tileId)
        {
            for (var index = 0; index < featureIds.Count; index++)
            {
                if (featureIds[index] != ushort.MaxValue && featureIds[index] >= buildingIds.Count)
                    throw new InvalidOperationException("GLB Feature ID is outside tile metadata features.buildingIds: " + tileId);
            }
        }

        private static Material CreateMaterial()
        {
            var shader = Shader.Find("CityModel/Feature Colors");
            if (shader == null) throw new InvalidOperationException("CityModel/Feature Colors shader is not available.");
            return new Material(shader);
        }

        private void OnDestroy()
        {
            _cancellation?.Cancel();
            _cancellation?.Dispose();
            _dataset?.Dispose();
            _colors?.Dispose();
            if (_tilesRoot != null) Destroy(_tilesRoot);
            if (_material != null) Destroy(_material);
        }
    }
}
