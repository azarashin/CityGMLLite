using System;
using System.Collections.Generic;
using CityModel.Georeference;
using UnityEngine;

namespace CityModel.Tiles
{
    public enum TileState { Unloaded, Loading, Loaded, Failed }
    public sealed class TileHandle { public string TileId; public ProjectedCoordinate Origin; public GameObject Root; public TileState State; }

    /// <summary>Tracks tile state and places loaded tile roots without changing GLB vertex buffers.</summary>
    public sealed class TileManager
    {
        private readonly Dictionary<string, TileHandle> _tiles = new();
        private readonly GeoReference _geoReference;
        public TileManager(GeoReference geoReference) { _geoReference = geoReference; }
        public IReadOnlyDictionary<string, TileHandle> Tiles => _tiles;
        public bool TryBeginLoad(string tileId, ProjectedCoordinate origin) { if (_tiles.TryGetValue(tileId, out var current) && current.State is TileState.Loading or TileState.Loaded) return false; _tiles[tileId] = new TileHandle { TileId = tileId, Origin = origin, State = TileState.Loading }; return true; }
        public void CompleteLoad(string tileId, GameObject root) { var tile = _tiles[tileId]; tile.Root = root; tile.State = TileState.Loaded; root.transform.position = _geoReference.ProjectedToUnity(tile.Origin); }
        public void RepositionLoadedTiles() { foreach (var tile in _tiles.Values) if (tile.State == TileState.Loaded && tile.Root != null) tile.Root.transform.position = _geoReference.ProjectedToUnity(tile.Origin); }
        public bool Unload(string tileId) { if (!_tiles.Remove(tileId, out var tile)) return false; if (tile.Root != null) UnityEngine.Object.Destroy(tile.Root); return true; }
    }
}
