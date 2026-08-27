using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

namespace CityModel.Database
{
    public sealed class BuildingRecord { public string BuildingId; public string CanonicalBuildingId; public string TileId; }
    public sealed class BuildingAttribute { public string Key; public string Value; public string Unit; public string CodeSpace; }
    public interface IReadOnlyBuildingStore : IDisposable { BuildingRecord FindBuilding(string buildingId); IReadOnlyList<BuildingAttribute> FindAttributes(string buildingId); }

    /// <summary>Safe Unity-facing read-only database API. The native bridge supplies only prepared operations.</summary>
    public sealed class BuildingDatabase : IDisposable
    {
        private readonly IReadOnlyBuildingStore _store;
        private BuildingDatabase(IReadOnlyBuildingStore store) { _store = store; }
        public static Task<BuildingDatabase> OpenAsync(string databasePath, Func<string, IReadOnlyBuildingStore> openReadOnly, CancellationToken cancellationToken)
        {
            if (Path.IsPathRooted(databasePath) || databasePath.Contains("..")) throw new InvalidDataException("Database path escapes the dataset root.");
            return Task.Run(() => { cancellationToken.ThrowIfCancellationRequested(); return new BuildingDatabase(openReadOnly(databasePath)); }, cancellationToken);
        }
        public Task<BuildingRecord> FindBuildingAsync(string buildingId, CancellationToken cancellationToken) => Task.Run(() => _store.FindBuilding(buildingId), cancellationToken);
        public Task<IReadOnlyList<BuildingAttribute>> FindAttributesAsync(string buildingId, CancellationToken cancellationToken) => Task.Run(() => _store.FindAttributes(buildingId), cancellationToken);
        public void Dispose() { _store.Dispose(); }
    }
}
