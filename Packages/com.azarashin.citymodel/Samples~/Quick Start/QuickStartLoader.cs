using System;
using System.Threading;
using System.Threading.Tasks;
using CityModel.Loading;
using UnityEngine;

namespace CityModel.Samples
{
    /// <summary>Minimal sample entry point. Set Dataset Root to a converter output directory.</summary>
    public sealed class QuickStartLoader : MonoBehaviour
    {
        [Tooltip("Absolute path to a directory containing dataset.manifest.json.")]
        [SerializeField] private string datasetRoot;

        private CityModelDataset _dataset;
        private CancellationTokenSource _cancellation;
        private GameObject _tilesRoot;
        private Material _material;

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
                Debug.Log($"CityModel dataset opened: {_dataset.Manifest.datasetId} ({_dataset.Manifest.generationId})");
                _tilesRoot = new GameObject("CityModel Tiles");
                _tilesRoot.transform.SetParent(transform, false);
                _material = CreateMaterial();
                foreach (var tile in _dataset.Manifest.tiles.items)
                {
                    _cancellation.Token.ThrowIfCancellationRequested();
                    var loadedTile = await _dataset.LoadTileAsync(tile, _cancellation.Token);
                    CreateTile(loadedTile);
                }
                Debug.Log($"CityModel Quick Start: rendered {_dataset.Manifest.tiles.items.Length} tile(s).", this);
            }
            catch (Exception exception)
            {
                Debug.LogError("CityModel Quick Start could not open or render the dataset. Check Dataset Root, dataset.manifest.json, and the converter output.\n" + exception.Message, this);
                Debug.LogException(exception, this);
            }
        }

        private void CreateTile(LoadedTile loadedTile)
        {
            if (loadedTile.Metadata.origin == null || loadedTile.Metadata.origin.projected == null || _dataset.Manifest.datasetOrigin == null || _dataset.Manifest.datasetOrigin.projected == null)
                throw new InvalidOperationException("Dataset or tile projected origin is missing.");

            var tile = new GameObject(loadedTile.Metadata.tileId);
            tile.transform.SetParent(_tilesRoot.transform, false);
            var tileOrigin = loadedTile.Metadata.origin.projected;
            var datasetOrigin = _dataset.Manifest.datasetOrigin.projected;
            tile.transform.localPosition = new Vector3(
                (float)(tileOrigin.x - datasetOrigin.x),
                (float)(tileOrigin.z - datasetOrigin.z),
                (float)(datasetOrigin.y - tileOrigin.y));

            var mesh = CityModelGlbDecoder.Decode(loadedTile.GlbBytes, loadedTile.Metadata.tileId);
            var filter = tile.AddComponent<MeshFilter>();
            filter.sharedMesh = mesh;
            var renderer = tile.AddComponent<MeshRenderer>();
            renderer.sharedMaterial = _material;
        }

        private static Material CreateMaterial()
        {
            var shader = Shader.Find("Universal Render Pipeline/Lit") ?? Shader.Find("Standard");
            if (shader == null) throw new InvalidOperationException("Neither Universal Render Pipeline/Lit nor Standard shader is available.");
            var material = new Material(shader) { color = new Color(0.68f, 0.78f, 0.92f) };
            return material;
        }

        private void OnDestroy()
        {
            _cancellation?.Cancel();
            _cancellation?.Dispose();
            _dataset?.Dispose();
            if (_tilesRoot != null) Destroy(_tilesRoot);
            if (_material != null) Destroy(_material);
        }
    }
}
