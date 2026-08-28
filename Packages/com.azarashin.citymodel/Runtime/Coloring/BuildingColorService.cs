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
        private static readonly int FeatureColorsId = Shader.PropertyToID("_CityModelFeatureColors");
        private static readonly int FeatureColorCountId = Shader.PropertyToID("_CityModelFeatureColorCount");
        private static readonly int DefaultColorId = Shader.PropertyToID("_CityModelDefaultColor");
        private readonly Dictionary<string, Color32> _colors = new();
        private readonly Dictionary<string, TileColorTable> _tiles = new();
        private readonly Color32 _defaultColor;
        public BuildingColorService(Color32 defaultColor) { _defaultColor = defaultColor; }
        public ColorUpdateResult SetColor(string buildingId, Color32 color) { _colors[buildingId] = color; var found = false; foreach (var table in _tiles.Values) found |= table.TrySet(buildingId, color); return found ? ColorUpdateResult.Applied : ColorUpdateResult.Deferred; }
        public void ClearColor(string buildingId) { _colors.Remove(buildingId); foreach (var table in _tiles.Values) table.TrySet(buildingId, _defaultColor); }
        public void ClearAll() { _colors.Clear(); foreach (var table in _tiles.Values) table.Reset(_defaultColor); }
        public void RegisterTile(string tileId, IReadOnlyList<string> buildingIds)
        {
            if (tileId == null) throw new ArgumentNullException(nameof(tileId));
            if (buildingIds == null) throw new ArgumentNullException(nameof(buildingIds));
            var table = new TileColorTable(buildingIds, _defaultColor);
            foreach (var pair in _colors) table.TrySet(pair.Key, pair.Value);
            if (_tiles.Remove(tileId, out var previous)) previous.Dispose();
            _tiles.Add(tileId, table);
        }

        /// <summary>Binds this tile's Feature ID color table directly to its dedicated material.</summary>
        public void ApplyToMaterial(string tileId, Material material)
        {
            if (material == null) throw new ArgumentNullException(nameof(material));
            if (!_tiles.TryGetValue(tileId, out var table)) throw new KeyNotFoundException("Tile color table is not registered: " + tileId);
            material.SetBuffer(FeatureColorsId, table.Buffer);
            material.SetInt(FeatureColorCountId, table.Count);
            material.SetColor(DefaultColorId, _defaultColor);
        }
        public void UnregisterTile(string tileId) { if (_tiles.Remove(tileId, out var table)) table.Dispose(); }
        public void Dispose() { foreach (var table in _tiles.Values) table.Dispose(); _tiles.Clear(); }
    }

    internal sealed class TileColorTable : IDisposable
    {
        private readonly Dictionary<string, int> _indices = new(); private readonly Vector4[] _colors; private readonly int _count; private GraphicsBuffer _buffer;
        public TileColorTable(IReadOnlyList<string> buildingIds, Color32 defaultColor) { _count = buildingIds.Count; _colors = new Vector4[Math.Max(1, _count)]; for (var index = 0; index < _colors.Length; index++) _colors[index] = ToVector4(defaultColor); for (var index = 0; index < _count; index++) _indices[buildingIds[index]] = index; _buffer = new GraphicsBuffer(GraphicsBuffer.Target.Structured, _colors.Length, sizeof(float) * 4); _buffer.SetData(_colors); }
        public GraphicsBuffer Buffer => _buffer;
        public int Count => _count;
        public bool TrySet(string buildingId, Color32 color) { if (!_indices.TryGetValue(buildingId, out var index)) return false; _colors[index] = ToVector4(color); _buffer.SetData(_colors, index, index, 1); return true; }
        public void Reset(Color32 color) { Array.Fill(_colors, ToVector4(color)); _buffer.SetData(_colors); }
        public void Dispose() { _buffer?.Dispose(); _buffer = null; }
        private static Vector4 ToVector4(Color32 color) { return new Vector4(color.r / 255f, color.g / 255f, color.b / 255f, color.a / 255f); }
    }
}
