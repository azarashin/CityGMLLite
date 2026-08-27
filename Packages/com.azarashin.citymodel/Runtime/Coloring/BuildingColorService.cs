using System;
using System.Collections.Generic;
using UnityEngine;

namespace CityModel.Coloring
{
    public enum ColorUpdateResult { Applied, Deferred, NotFound }
    public interface IBuildingColorService { ColorUpdateResult SetColor(string buildingId, Color32 color); void ClearColor(string buildingId); void ClearAll(); }

    /// <summary>Persists BuildingID colors and uploads only dirty Feature ID slots for loaded tiles.</summary>
    public sealed class BuildingColorService : IBuildingColorService, IDisposable
    {
        private readonly Dictionary<string, Color32> _colors = new();
        private readonly Dictionary<string, TileColorTable> _tiles = new();
        private readonly Color32 _defaultColor;
        public BuildingColorService(Color32 defaultColor) { _defaultColor = defaultColor; }
        public ColorUpdateResult SetColor(string buildingId, Color32 color) { _colors[buildingId] = color; var found = false; foreach (var table in _tiles.Values) found |= table.TrySet(buildingId, color); return found ? ColorUpdateResult.Applied : ColorUpdateResult.Deferred; }
        public void ClearColor(string buildingId) { _colors.Remove(buildingId); foreach (var table in _tiles.Values) table.TrySet(buildingId, _defaultColor); }
        public void ClearAll() { _colors.Clear(); foreach (var table in _tiles.Values) table.Reset(_defaultColor); }
        public void RegisterTile(string tileId, IReadOnlyList<string> buildingIds) { var table = new TileColorTable(buildingIds, _defaultColor); foreach (var pair in _colors) table.TrySet(pair.Key, pair.Value); _tiles[tileId] = table; }
        public void UnregisterTile(string tileId) { if (_tiles.Remove(tileId, out var table)) table.Dispose(); }
        public void Dispose() { foreach (var table in _tiles.Values) table.Dispose(); _tiles.Clear(); }
    }

    internal sealed class TileColorTable : IDisposable
    {
        private readonly Dictionary<string, int> _indices = new(); private readonly Color32[] _colors; private GraphicsBuffer _buffer;
        public TileColorTable(IReadOnlyList<string> buildingIds, Color32 defaultColor) { _colors = new Color32[buildingIds.Count]; for (var index = 0; index < buildingIds.Count; index++) { _indices[buildingIds[index]] = index; _colors[index] = defaultColor; } _buffer = new GraphicsBuffer(GraphicsBuffer.Target.Structured, Math.Max(1, _colors.Length), 4); _buffer.SetData(_colors); }
        public bool TrySet(string buildingId, Color32 color) { if (!_indices.TryGetValue(buildingId, out var index)) return false; _colors[index] = color; _buffer.SetData(_colors, index, index, 1); return true; }
        public void Reset(Color32 color) { Array.Fill(_colors, color); _buffer.SetData(_colors); }
        public void Dispose() { _buffer?.Dispose(); _buffer = null; }
    }
}
