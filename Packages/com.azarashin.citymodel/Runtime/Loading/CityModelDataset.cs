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
        public ManifestTileContent[] contents;
    }

    /// <summary>
    /// A type-specific metadata index. It lets callers decide what to load
    /// before opening the metadata or GLB artifact.
    /// </summary>
    [Serializable]
    public sealed class ManifestTileContent
    {
        public string featureType;
        public string metadata;
        public string sha256;
        public long byteLength;
    }

    [Serializable]
    public sealed class TileContent
    {
        public string featureType;
        public string glb;
        public string sha256;
        public long byteLength;
    }

    [Serializable]
    public sealed class TileFeatures
    {
        public string semantic;
        public string componentType;
        public int nullFeatureId;
        public string[] buildingIds;
        public GenericTileFeature[] items;
    }

    [Serializable]
    public sealed class GenericTileFeature
    {
        public int localFeatureId;
        public string featureId;
        public string featureType;
    }

    [Serializable]
    public sealed class TileMetadata
    {
        public string schemaVersion;
        public string generationId;
        public string tileId;
        public TileContent content;
        public TileFeatures features;
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
            return await LoadTileFromMetadataAsync(tile.metadata, tile.tileId, null, -1, cancellationToken).ConfigureAwait(false);
        }

        /// <summary>Returns content indexes of the requested type without performing artifact I/O.</summary>
        public ManifestTileContent[] GetContents(string featureType)
        {
            if (string.IsNullOrWhiteSpace(featureType)) throw new ArgumentException("Feature type is required.", nameof(featureType));
            var matches = new System.Collections.Generic.List<ManifestTileContent>();
            foreach (var tile in Manifest.tiles.items)
            {
                if (tile.contents == null) continue; // v1 manifests remain loadable through LoadTileAsync.
                foreach (var content in tile.contents)
                    if (content != null && string.Equals(content.featureType, featureType, StringComparison.Ordinal)) matches.Add(content);
            }
            return matches.ToArray();
        }

        /// <summary>Loads only the metadata and GLB declared by one type-specific content index.</summary>
        public Task<LoadedTile> LoadContentAsync(ManifestTile tile, ManifestTileContent content, CancellationToken cancellationToken)
        {
            if (tile == null || content == null || string.IsNullOrWhiteSpace(content.featureType))
                throw new InvalidDataException("Tile content index is incomplete.");
            ValidateSha256(content.sha256, "Tile content index SHA-256 is invalid.");
            if (content.byteLength < 0 || string.IsNullOrWhiteSpace(content.metadata))
                throw new InvalidDataException("Tile content index is incomplete.");
            return LoadContentFromMetadataAsync(tile, content, cancellationToken);
        }

        private async Task<LoadedTile> LoadContentFromMetadataAsync(ManifestTile tile, ManifestTileContent content, CancellationToken cancellationToken)
        {
            var loaded = await LoadTileFromMetadataAsync(content.metadata, tile.tileId, content.sha256, content.byteLength, cancellationToken).ConfigureAwait(false);
            if (loaded.Metadata.content == null || !string.Equals(loaded.Metadata.content.featureType, content.featureType, StringComparison.Ordinal))
                throw new InvalidDataException("Tile metadata feature type does not match the content index.");
            return loaded;
        }

        private async Task<LoadedTile> LoadTileFromMetadataAsync(string metadataRelativePath, string expectedTileId, string expectedMetadataSha256, long expectedMetadataLength, CancellationToken cancellationToken)
        {
            ThrowIfDisposed();
            await _loadGate.WaitAsync(cancellationToken).ConfigureAwait(false);
            try
            {
                var metadataPath = ResolveRelativePath(metadataRelativePath);
                var metadataBytes = await File.ReadAllBytesAsync(metadataPath, cancellationToken).ConfigureAwait(false);
                if (expectedMetadataLength >= 0 && metadataBytes.LongLength != expectedMetadataLength)
                    throw new InvalidDataException("Tile metadata length does not match the content index.");
                if (!string.IsNullOrEmpty(expectedMetadataSha256) && !string.Equals(ToSha256(metadataBytes), expectedMetadataSha256, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("Tile metadata SHA-256 does not match the content index.");
                var metadataJson = System.Text.Encoding.UTF8.GetString(metadataBytes);
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
            return (await LoadTileFromMetadataAsync(metadataRelativePath, null, null, -1, cancellationToken).ConfigureAwait(false)).GlbBytes;
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
        private static void ValidateSha256(string value, string message)
        {
            if (string.IsNullOrEmpty(value) || value.Length != 64) throw new InvalidDataException(message);
            foreach (var character in value)
                if (!Uri.IsHexDigit(character)) throw new InvalidDataException(message);
        }
        private void ThrowIfDisposed() { if (_disposed) throw new ObjectDisposedException(nameof(CityModelDataset)); }
    }
}
