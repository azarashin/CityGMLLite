using System;
using System.Threading;
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
                _tilesRoot = new GameObject("CityModel Tiles");
                _tilesRoot.transform.SetParent(transform, false);
                _material = CreateMaterial();
                foreach (var tile in _dataset.Manifest.tiles.items)
                {
                    _cancellation.Token.ThrowIfCancellationRequested();
                    CreateTile(await _dataset.LoadTileAsync(tile, _cancellation.Token));
                }

                Debug.Log($"CityModel Quick Start: rendered {_dataset.Manifest.tiles.items.Length} tile(s).", this);
            }
            catch (Exception exception)
            {
                Debug.LogException(exception, this);
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
            tile.AddComponent<MeshFilter>().sharedMesh = CityModelGlbDecoder.Decode(loadedTile.GlbBytes, loadedTile.Metadata.tileId);
            tile.AddComponent<MeshRenderer>().sharedMaterial = _material;
        }

        private static Material CreateMaterial()
        {
            var shader = Shader.Find("Universal Render Pipeline/Lit") ?? Shader.Find("Standard");
            if (shader == null) throw new InvalidOperationException("No supported lit shader is available.");
            return new Material(shader) { color = new Color(0.68f, 0.78f, 0.92f) };
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
