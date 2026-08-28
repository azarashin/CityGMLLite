using System;
using System.Collections.Generic;
using System.Threading;
using CityModel.Coloring;
using CityModel.Database;
using CityModel.Loading;
using CityModel.Picking;
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

        [Tooltip("Each type can be loaded into memory independently. Initially Visible has no effect when Load On Startup is disabled.")]
        [SerializeField] private CityModelFeatureTypeStartupSetting[] featureTypeStartupSettings =
        {
            new CityModelFeatureTypeStartupSetting
            {
                featureType = CityModelFeatureTypes.Building,
                loadOnStartup = true,
                initiallyVisible = true,
            },
        };

        private CityModelDataset _dataset;
        private CancellationTokenSource _cancellation;
        private GameObject _tilesRoot;
        private Material _material;
        private Material _terrainMaterial;
        private BuildingColorService _colors;
        private readonly List<Material> _tileMaterials = new List<Material>();
        private readonly List<Mesh> _tileMeshes = new List<Mesh>();
        private readonly List<Texture2D> _tileTextures = new List<Texture2D>();
        private readonly Dictionary<string, GameObject> _typeRoots = new Dictionary<string, GameObject>(StringComparer.OrdinalIgnoreCase);
        private readonly Dictionary<string, List<MeshRenderer>> _typeRenderers = new Dictionary<string, List<MeshRenderer>>(StringComparer.OrdinalIgnoreCase);

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
                var loadedContents = await LoadConfiguredContentsAsync(_cancellation.Token);
                if (loadedContents.Count > 0) _material = CreateMaterial();
                foreach (var loadedContent in loadedContents)
                    if (IsTerrain(loadedContent.FeatureType)) { _terrainMaterial = CreateTerrainMaterial(); break; }
                var loadedBuildings = new List<LoadedTile>();
                foreach (var loadedContent in loadedContents)
                    if (IsBuilding(loadedContent.FeatureType)) loadedBuildings.Add(loadedContent.Tile);

                if (loadedBuildings.Count > 0)
                {
                    _colors = new BuildingColorService(BuildingAttributeColorizer.MissingAttributeColor);
                    await ApplyAttributeColorsAsync(loadedBuildings, _cancellation.Token);
                }

                foreach (var loadedContent in loadedContents)
                    CreateTile(loadedContent.Tile, loadedContent.FeatureType, loadedContent.InitiallyVisible);

                Debug.Log($"CityModel Quick Start: loaded {loadedContents.Count} content item(s).", this);
            }
            catch (Exception exception)
            {
                Debug.LogException(exception, this);
            }
        }

        /// <summary>
        /// Shows or hides a loaded feature type without decoding its artifacts again.
        /// Returns false when that type was not loaded at startup.
        /// </summary>
        public bool SetFeatureTypeVisible(string featureType, bool visible)
        {
            if (string.IsNullOrWhiteSpace(featureType)) throw new ArgumentException("Feature type is required.", nameof(featureType));
            if (!_typeRenderers.TryGetValue(featureType.Trim(), out var renderers)) return false;
            foreach (var renderer in renderers)
                if (renderer != null) renderer.enabled = visible;
            return true;
        }

        /// <summary>Returns whether this type has instantiated render resources.</summary>
        public bool IsFeatureTypeLoaded(string featureType)
        {
            return !string.IsNullOrWhiteSpace(featureType) && _typeRenderers.ContainsKey(featureType.Trim());
        }

        private async System.Threading.Tasks.Task<List<LoadedFeatureContent>> LoadConfiguredContentsAsync(CancellationToken cancellationToken)
        {
            var loadedContents = new List<LoadedFeatureContent>();
            if (featureTypeStartupSettings == null || featureTypeStartupSettings.Length == 0)
            {
                // Scenes saved before the setting was introduced must keep loading their
                // building-only artifacts even when Unity deserializes the new field as null.
                foreach (var manifestTile in _dataset.Manifest.tiles.items)
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    loadedContents.Add(new LoadedFeatureContent(
                        CityModelFeatureTypes.Building,
                        true,
                        await _dataset.LoadTileAsync(manifestTile, cancellationToken)));
                }
                return loadedContents;
            }

            var configuredTypes = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            foreach (var setting in featureTypeStartupSettings)
            {
                if (setting == null || string.IsNullOrWhiteSpace(setting.featureType)) continue;
                var featureType = setting.featureType.Trim();
                if (!configuredTypes.Add(featureType)) continue;
                if (!CityModelFeatureTypeStartupSettings.TryResolve(featureTypeStartupSettings, featureType, out var loadOnStartup, out var initiallyVisible) || !loadOnStartup)
                    continue;

                var contentIndexes = _dataset.GetContents(featureType);
                if (contentIndexes.Length > 0)
                {
                    foreach (var manifestTile in _dataset.Manifest.tiles.items)
                    {
                        if (manifestTile.contents == null) continue;
                        foreach (var content in manifestTile.contents)
                        {
                            if (content == null || !string.Equals(content.featureType, featureType, StringComparison.OrdinalIgnoreCase)) continue;
                            cancellationToken.ThrowIfCancellationRequested();
                            loadedContents.Add(new LoadedFeatureContent(
                                featureType,
                                initiallyVisible,
                                await _dataset.LoadContentAsync(manifestTile, content, cancellationToken)));
                        }
                    }
                }
                else if (IsBuilding(featureType))
                {
                    // Pre-type-index artifacts only contain buildings. Preserve their existing
                    // behaviour without allowing other unindexed types to trigger artifact I/O.
                    foreach (var manifestTile in _dataset.Manifest.tiles.items)
                    {
                        if (manifestTile.contents != null) continue;
                        cancellationToken.ThrowIfCancellationRequested();
                        loadedContents.Add(new LoadedFeatureContent(
                            CityModelFeatureTypes.Building,
                            initiallyVisible,
                            await _dataset.LoadTileAsync(manifestTile, cancellationToken)));
                    }
                }
            }
            return loadedContents;
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

        private void CreateTile(LoadedTile loadedTile, string featureType, bool initiallyVisible)
        {
            var tileOrigin = loadedTile.Metadata.origin?.projected;
            var datasetOrigin = _dataset.Manifest.datasetOrigin?.projected;
            if (tileOrigin == null || datasetOrigin == null)
                throw new InvalidOperationException("Dataset or tile projected origin is missing.");

            var tile = new GameObject(loadedTile.Metadata.tileId);
            tile.transform.SetParent(GetOrCreateTypeRoot(featureType).transform, false);
            tile.transform.localPosition = new Vector3(
                (float)(tileOrigin.x - datasetOrigin.x),
                (float)(tileOrigin.z - datasetOrigin.z),
                (float)(datasetOrigin.y - tileOrigin.y));
            var decoded = CityModelGlbDecoder.DecodeWithFeatureIds(loadedTile.GlbBytes, loadedTile.Metadata.tileId);
            _tileMeshes.Add(decoded.Mesh);
            tile.AddComponent<MeshFilter>().sharedMesh = decoded.Mesh;
            // Keep the collider active even when the renderer starts hidden: visibility
            // controls drawing only, while FeaturePicker resolves loaded mesh triangles.
            tile.AddComponent<MeshCollider>().sharedMesh = decoded.Mesh;
            var featureBinding = tile.AddComponent<CityModelTileFeatureBinding>();
            featureBinding.Initialize(loadedTile.Metadata.tileId, loadedTile.Metadata.features?.items, decoded.TriangleFeatureIds);
            var renderer = tile.AddComponent<MeshRenderer>();
            if (!_typeRenderers.TryGetValue(featureType, out var renderers))
            {
                renderers = new List<MeshRenderer>();
                _typeRenderers.Add(featureType, renderers);
            }
            renderers.Add(renderer);
            Material[] tileMaterials;
            if (IsBuilding(featureType))
            {
                var tileMaterial = new Material(_material)
                {
                    name = _material.name + " (" + loadedTile.Metadata.tileId + ")",
                };
                var buildingIds = loadedTile.Metadata.features?.buildingIds ?? Array.Empty<string>();
                ValidateFeatureIds(decoded.FeatureIds, buildingIds, loadedTile.Metadata.tileId);
                _colors.RegisterTile(loadedTile.Metadata.tileId, buildingIds);
                _colors.ApplyToMaterial(loadedTile.Metadata.tileId, tileMaterial);
                tileMaterials = new[] { tileMaterial };
            }
            else if (IsTerrain(featureType) && decoded.HasEmbeddedTextures)
            {
                ValidateGenericFeatureIds(decoded.FeatureIds, loadedTile.Metadata.features?.items, featureType, loadedTile.Metadata.tileId);
                tileMaterials = new Material[decoded.Textures.Length];
                for (var index = 0; index < tileMaterials.Length; index++)
                {
                    var tileMaterial = new Material(_terrainMaterial)
                    {
                        name = _terrainMaterial.name + " (" + loadedTile.Metadata.tileId + ")",
                    };
                    tileMaterial.mainTexture = decoded.Textures[index];
                    tileMaterials[index] = tileMaterial;
                    _tileMaterials.Add(tileMaterial);
                    _tileTextures.Add(decoded.Textures[index]);
                }
            }
            else
            {
                var tileMaterial = new Material(_material)
                {
                    name = _material.name + " (" + loadedTile.Metadata.tileId + ")",
                };
                tileMaterials = new[] { tileMaterial };
            }
            renderer.sharedMaterials = tileMaterials;
            renderer.enabled = initiallyVisible;
            if (!IsTerrain(featureType) || !decoded.HasEmbeddedTextures)
                _tileMaterials.AddRange(tileMaterials);
        }

        private GameObject GetOrCreateTypeRoot(string featureType)
        {
            if (_typeRoots.TryGetValue(featureType, out var typeRoot)) return typeRoot;
            if (_tilesRoot == null)
            {
                _tilesRoot = new GameObject("CityModel Tiles");
                _tilesRoot.transform.SetParent(transform, false);
            }
            typeRoot = new GameObject(featureType + " Tiles");
            typeRoot.transform.SetParent(_tilesRoot.transform, false);
            _typeRoots.Add(featureType, typeRoot);
            return typeRoot;
        }

        private static bool IsBuilding(string featureType)
        {
            return string.Equals(featureType, CityModelFeatureTypes.Building, StringComparison.OrdinalIgnoreCase);
        }

        private static bool IsTerrain(string featureType)
        {
            return string.Equals(featureType, CityModelFeatureTypes.Terrain, StringComparison.OrdinalIgnoreCase);
        }

        private static void ValidateFeatureIds(IReadOnlyList<ushort> featureIds, IReadOnlyList<string> buildingIds, string tileId)
        {
            for (var index = 0; index < featureIds.Count; index++)
            {
                if (featureIds[index] != ushort.MaxValue && featureIds[index] >= buildingIds.Count)
                    throw new InvalidOperationException("GLB Feature ID is outside tile metadata features.buildingIds: " + tileId);
            }
        }

        private static void ValidateGenericFeatureIds(IReadOnlyList<ushort> featureIds, IReadOnlyList<GenericTileFeature> features, string featureType, string tileId)
        {
            for (var index = 0; index < featureIds.Count; index++)
            {
                if (featureIds[index] == ushort.MaxValue) continue;
                if (!CityModel.Picking.FeaturePicker.TryResolveFeature(features, featureIds[index], out var feature) || !string.Equals(feature.featureType, featureType, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidOperationException("GLB Feature ID does not resolve to the expected generic tile feature: " + tileId);
            }
        }

        private static Material CreateMaterial()
        {
            var shader = Shader.Find("CityModel/Feature Colors");
            if (shader == null) throw new InvalidOperationException("CityModel/Feature Colors shader is not available.");
            return new Material(shader);
        }

        private static Material CreateTerrainMaterial()
        {
            var shader = Shader.Find("CityModel/Terrain Textured");
            if (shader == null) throw new InvalidOperationException("CityModel/Terrain Textured shader is not available.");
            return new Material(shader);
        }

        private void OnDestroy()
        {
            _cancellation?.Cancel();
            _cancellation?.Dispose();
            _dataset?.Dispose();
            _colors?.Dispose();
            foreach (var tileMaterial in _tileMaterials) Destroy(tileMaterial);
            _tileMaterials.Clear();
            foreach (var tileMesh in _tileMeshes) Destroy(tileMesh);
            _tileMeshes.Clear();
            foreach (var tileTexture in _tileTextures) Destroy(tileTexture);
            _tileTextures.Clear();
            _typeRenderers.Clear();
            _typeRoots.Clear();
            if (_tilesRoot != null) Destroy(_tilesRoot);
            if (_material != null) Destroy(_material);
            if (_terrainMaterial != null) Destroy(_terrainMaterial);
        }

        private sealed class LoadedFeatureContent
        {
            public LoadedFeatureContent(string featureType, bool initiallyVisible, LoadedTile tile)
            {
                FeatureType = featureType;
                InitiallyVisible = initiallyVisible;
                Tile = tile;
            }

            public string FeatureType { get; }
            public bool InitiallyVisible { get; }
            public LoadedTile Tile { get; }
        }
    }
}
