using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using System.Security.Cryptography;
using CityModel.Loading;
using CityModel.Versioning;

namespace CityModel.Database
{
    public sealed class BuildingRecord { public string BuildingId; public string CanonicalBuildingId; public string TileId; }
    public sealed class BuildingAttribute { public string Key; public string Value; public string Unit; public string CodeSpace; }
    public interface IReadOnlyBuildingStore : IDisposable { BuildingRecord FindBuilding(string buildingId); IReadOnlyList<BuildingAttribute> FindAttributes(string buildingId); }

    /// <summary>Safe Unity-facing read-only database API. The native bridge supplies only prepared operations.</summary>
    public sealed class BuildingDatabase : IDisposable
    {
        private readonly IReadOnlyBuildingStore _store;
        private bool _disposed;
        private BuildingDatabase(IReadOnlyBuildingStore store) { _store = store; }

        /// <summary>Opens and validates the manifest-declared SQLite database using fixed, read-only queries.</summary>
        public static Task<BuildingDatabase> OpenAsync(string datasetRoot, DatasetManifest manifest, CancellationToken cancellationToken)
        {
            if (manifest == null || manifest.database == null) throw new InvalidDataException("Manifest database information is missing.");
            return Task.Run(() => OpenValidated(datasetRoot, manifest, cancellationToken), cancellationToken);
        }

        public static Task<BuildingDatabase> OpenAsync(string databasePath, Func<string, IReadOnlyBuildingStore> openReadOnly, CancellationToken cancellationToken)
        {
            if (Path.IsPathRooted(databasePath) || databasePath.Contains("..")) throw new InvalidDataException("Database path escapes the dataset root.");
            return Task.Run(() => { cancellationToken.ThrowIfCancellationRequested(); return new BuildingDatabase(openReadOnly(databasePath)); }, cancellationToken);
        }
        public Task<BuildingRecord> FindBuildingAsync(string buildingId, CancellationToken cancellationToken) => Task.Run(() => { ThrowIfDisposed(); return _store.FindBuilding(buildingId); }, cancellationToken);
        public Task<IReadOnlyList<BuildingAttribute>> FindAttributesAsync(string buildingId, CancellationToken cancellationToken) => Task.Run(() => { ThrowIfDisposed(); return _store.FindAttributes(buildingId); }, cancellationToken);
        public void Dispose() { if (!_disposed) { _disposed = true; _store.Dispose(); } }

        private static BuildingDatabase OpenValidated(string datasetRoot, DatasetManifest manifest, CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (manifest.schemaVersion != CityModelContractVersion.SchemaVersion || string.IsNullOrWhiteSpace(manifest.generationId))
                throw new InvalidDataException("Manifest schemaVersion or generationId is invalid.");
            var databasePath = ResolveDatabasePath(datasetRoot, manifest.database.path);
            ValidateSha256(manifest.database.sha256, "Manifest database SHA-256 is invalid.");
            if (!File.Exists(databasePath)) throw new FileNotFoundException("Manifest-declared SQLite database was not found.", databasePath);
            if (!string.Equals(ToSha256(File.ReadAllBytes(databasePath)), manifest.database.sha256, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("SQLite SHA-256 does not match the dataset manifest.");

            SqliteReadOnlyBuildingStore store = null;
            try
            {
                store = SqliteReadOnlyBuildingStore.Open(databasePath);
                var metadata = store.ReadMetadata();
                if (metadata.SchemaVersion != CityModelContractVersion.SchemaVersion || metadata.GenerationId != manifest.generationId)
                    throw new InvalidDataException("SQLite metadata does not match the dataset manifest.");
                // database_sha256 inside the database is historical placeholder data in v1 artifacts;
                // the manifest hash above is the authoritative integrity check.
                if (!string.IsNullOrEmpty(metadata.DatabaseSha256) && !IsAllZeroHash(metadata.DatabaseSha256) && !string.Equals(metadata.DatabaseSha256, manifest.database.sha256, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("SQLite metadata hash does not match the dataset manifest.");
                return new BuildingDatabase(store);
            }
            catch { if (store != null) store.Dispose(); throw; }
        }

        private static string ResolveDatabasePath(string datasetRoot, string relativePath)
        {
            if (string.IsNullOrWhiteSpace(datasetRoot) || string.IsNullOrWhiteSpace(relativePath) || Path.IsPathRooted(relativePath))
                throw new InvalidDataException("Database path escapes the dataset root.");
            var root = Path.GetFullPath(datasetRoot);
            var candidate = Path.GetFullPath(Path.Combine(root, relativePath));
            var rootWithSeparator = root.EndsWith(Path.DirectorySeparatorChar.ToString(), StringComparison.Ordinal) ? root : root + Path.DirectorySeparatorChar;
            if (!candidate.StartsWith(rootWithSeparator, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("Database path escapes the dataset root.");
            return candidate;
        }

        private static void ValidateSha256(string value, string message)
        {
            if (string.IsNullOrEmpty(value) || value.Length != 64) throw new InvalidDataException(message);
            for (var i = 0; i < value.Length; i++) if (!Uri.IsHexDigit(value[i])) throw new InvalidDataException(message);
        }

        private static bool IsAllZeroHash(string value)
        {
            if (value.Length != 64) return false;
            for (var i = 0; i < value.Length; i++) if (value[i] != '0') return false;
            return true;
        }

        private static string ToSha256(byte[] bytes)
        {
            using (var hash = SHA256.Create()) return BitConverter.ToString(hash.ComputeHash(bytes)).Replace("-", string.Empty).ToLowerInvariant();
        }

        private void ThrowIfDisposed() { if (_disposed) throw new ObjectDisposedException(nameof(BuildingDatabase)); }
    }
}
