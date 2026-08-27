using System;
using System.IO;
using System.Security.Cryptography;
using System.Threading;
using System.Threading.Tasks;
using CityModel.Versioning;
using UnityEngine;

namespace CityModel.Loading
{
    [Serializable]
    public sealed class DatasetManifest
    {
        public string schemaVersion;
        public string datasetId;
        public string generationId;
    }

    [Serializable]
    public sealed class TileContent
    {
        public string glb;
        public string sha256;
    }

    [Serializable]
    public sealed class TileMetadata
    {
        public string schemaVersion;
        public string generationId;
        public string tileId;
        public TileContent content;
    }

    /// <summary>Owns asynchronous opening, validation, and cleanup of one generated dataset.</summary>
    public sealed class CityModelDataset : IDisposable
    {
        private readonly string _rootDirectory;
        private readonly SemaphoreSlim _loadGate;
        private bool _disposed;

        private CityModelDataset(string rootDirectory, DatasetManifest manifest, int maxConcurrentLoads)
        {
            _rootDirectory = rootDirectory;
            Manifest = manifest;
            _loadGate = new SemaphoreSlim(Math.Max(1, maxConcurrentLoads));
        }

        public DatasetManifest Manifest { get; }

        public static async Task<CityModelDataset> OpenAsync(string rootDirectory, CancellationToken cancellationToken, int maxConcurrentLoads = 2)
        {
            var manifestPath = Path.Combine(rootDirectory, "dataset.manifest.json");
            var json = await File.ReadAllTextAsync(manifestPath, cancellationToken).ConfigureAwait(false);
            var manifest = JsonUtility.FromJson<DatasetManifest>(json) ?? throw new InvalidDataException("Manifest cannot be parsed.");
            if (manifest.schemaVersion != CityModelContractVersion.SchemaVersion || string.IsNullOrWhiteSpace(manifest.generationId))
                throw new InvalidDataException("Manifest schemaVersion or generationId is invalid.");
            return new CityModelDataset(rootDirectory, manifest, maxConcurrentLoads);
        }

        public async Task<byte[]> LoadGlbAsync(string metadataRelativePath, CancellationToken cancellationToken)
        {
            ThrowIfDisposed();
            await _loadGate.WaitAsync(cancellationToken).ConfigureAwait(false);
            try
            {
                var metadataPath = ResolveRelativePath(metadataRelativePath);
                var metadataJson = await File.ReadAllTextAsync(metadataPath, cancellationToken).ConfigureAwait(false);
                var metadata = JsonUtility.FromJson<TileMetadata>(metadataJson) ?? throw new InvalidDataException("Tile metadata cannot be parsed.");
                if (metadata.generationId != Manifest.generationId)
                    throw new InvalidDataException("Tile generationId does not match the manifest.");
                var bytes = await File.ReadAllBytesAsync(ResolveRelativePath(metadata.content.glb), cancellationToken).ConfigureAwait(false);
                if (!string.Equals(ToSha256(bytes), metadata.content.sha256, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("GLB SHA-256 does not match tile metadata.");
                return bytes;
            }
            finally { _loadGate.Release(); }
        }

        public void Dispose() { if (!_disposed) { _loadGate.Dispose(); _disposed = true; } }
        private string ResolveRelativePath(string relativePath)
        {
            if (Path.IsPathRooted(relativePath) || relativePath.Contains("..")) throw new InvalidDataException("Dataset path escapes its root.");
            return Path.Combine(_rootDirectory, relativePath);
        }
        private static string ToSha256(byte[] bytes) { using var hash = SHA256.Create(); return BitConverter.ToString(hash.ComputeHash(bytes)).Replace("-", string.Empty).ToLowerInvariant(); }
        private void ThrowIfDisposed() { if (_disposed) throw new ObjectDisposedException(nameof(CityModelDataset)); }
    }
}
