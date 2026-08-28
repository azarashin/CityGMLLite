using System;
using System.IO;
using System.Security.Cryptography;
using System.Threading;
using System.Threading.Tasks;
using CityModel.Database;
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
        public ManifestDatabase database;
        public ProjectedOrigin datasetOrigin;
        public ManifestTiles tiles;
    }

    [Serializable]
    public sealed class ManifestDatabase
    {
        public string path;
        public string sha256;
    }

    [Serializable]
    public sealed class ProjectedOrigin
    {
        public ProjectedCoordinate projected;
    }

    [Serializable]
    public sealed class ProjectedCoordinate
    {
        public double x;
        public double y;
        public double z;
        public int epsg;
    }

    [Serializable]
    public sealed class ManifestTiles
    {
        public string indexType;
        public ManifestTile[] items;
    }

    [Serializable]
    public sealed class ManifestTile
    {
        public string tileId;
        public string metadata;
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
        public ProjectedOrigin origin;
    }

    /// <summary>Validated tile metadata and the corresponding GLB bytes.</summary>
    public sealed class LoadedTile
    {
        public LoadedTile(TileMetadata metadata, byte[] glbBytes)
        {
            Metadata = metadata;
            GlbBytes = glbBytes;
        }

        public TileMetadata Metadata { get; }
        public byte[] GlbBytes { get; }
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
            if (string.IsNullOrWhiteSpace(rootDirectory)) throw new ArgumentException("Dataset root is required.", nameof(rootDirectory));
            rootDirectory = Path.GetFullPath(rootDirectory);
            var manifestPath = Path.Combine(rootDirectory, "dataset.manifest.json");
            var json = await File.ReadAllTextAsync(manifestPath, cancellationToken).ConfigureAwait(false);
            var manifest = JsonUtility.FromJson<DatasetManifest>(json) ?? throw new InvalidDataException("Manifest cannot be parsed.");
            if (manifest.schemaVersion != CityModelContractVersion.SchemaVersion || string.IsNullOrWhiteSpace(manifest.generationId))
                throw new InvalidDataException("Manifest schemaVersion or generationId is invalid.");
            if (manifest.tiles == null || manifest.tiles.indexType != "inline" || manifest.tiles.items == null)
                throw new InvalidDataException("Only manifests with an inline tile index are supported.");
            return new CityModelDataset(rootDirectory, manifest, maxConcurrentLoads);
        }

        /// <summary>Opens the dataset's manifest-declared SQLite artifact through the read-only Windows bridge.</summary>
        public Task<BuildingDatabase> OpenBuildingDatabaseAsync(CancellationToken cancellationToken)
        {
            ThrowIfDisposed();
            return BuildingDatabase.OpenAsync(_rootDirectory, Manifest, cancellationToken);
        }

        public async Task<LoadedTile> LoadTileAsync(ManifestTile tile, CancellationToken cancellationToken)
        {
            if (tile == null || string.IsNullOrWhiteSpace(tile.metadata))
                throw new InvalidDataException("Tile metadata path is missing.");
            return await LoadTileFromMetadataAsync(tile.metadata, tile.tileId, cancellationToken).ConfigureAwait(false);
        }

        private async Task<LoadedTile> LoadTileFromMetadataAsync(string metadataRelativePath, string expectedTileId, CancellationToken cancellationToken)
        {
            ThrowIfDisposed();
            await _loadGate.WaitAsync(cancellationToken).ConfigureAwait(false);
            try
            {
                var metadataPath = ResolveRelativePath(metadataRelativePath);
                var metadataJson = await File.ReadAllTextAsync(metadataPath, cancellationToken).ConfigureAwait(false);
                var metadata = JsonUtility.FromJson<TileMetadata>(metadataJson) ?? throw new InvalidDataException("Tile metadata cannot be parsed.");
                if (metadata.schemaVersion != CityModelContractVersion.SchemaVersion || metadata.generationId != Manifest.generationId || (!string.IsNullOrEmpty(expectedTileId) && metadata.tileId != expectedTileId))
                    throw new InvalidDataException("Tile metadata does not match the dataset manifest.");
                if (metadata.content == null || string.IsNullOrWhiteSpace(metadata.content.glb) || string.IsNullOrWhiteSpace(metadata.content.sha256))
                    throw new InvalidDataException("Tile content is incomplete.");

                // The dataset contract stores content.glb relative to the dataset root,
                // rather than relative to the tile metadata file.
                var glbPath = ResolveRelativePath(metadata.content.glb);
                var bytes = await File.ReadAllBytesAsync(glbPath, cancellationToken).ConfigureAwait(false);
                if (!string.Equals(ToSha256(bytes), metadata.content.sha256, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("GLB SHA-256 does not match tile metadata.");
                return new LoadedTile(metadata, bytes);
            }
            finally { _loadGate.Release(); }
        }

        public async Task<byte[]> LoadGlbAsync(string metadataRelativePath, CancellationToken cancellationToken)
        {
            return (await LoadTileFromMetadataAsync(metadataRelativePath, null, cancellationToken).ConfigureAwait(false)).GlbBytes;
        }

        public void Dispose() { if (!_disposed) { _loadGate.Dispose(); _disposed = true; } }
        private string ResolveRelativePath(string relativePath)
        {
            if (string.IsNullOrWhiteSpace(relativePath) || Path.IsPathRooted(relativePath)) throw new InvalidDataException("Dataset path escapes its root.");
            var root = Path.GetFullPath(_rootDirectory);
            var candidate = Path.GetFullPath(Path.Combine(root, relativePath));
            var rootWithSeparator = root.EndsWith(Path.DirectorySeparatorChar.ToString(), StringComparison.Ordinal)
                ? root
                : root + Path.DirectorySeparatorChar;
            if (!candidate.StartsWith(rootWithSeparator, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("Dataset path escapes its root.");
            return candidate;
        }
        private static string ToSha256(byte[] bytes) { using var hash = SHA256.Create(); return BitConverter.ToString(hash.ComputeHash(bytes)).Replace("-", string.Empty).ToLowerInvariant(); }
        private void ThrowIfDisposed() { if (_disposed) throw new ObjectDisposedException(nameof(CityModelDataset)); }
    }
}
