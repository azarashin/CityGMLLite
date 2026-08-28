using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using CityModel.Database;
using NUnit.Framework;

namespace CityModel.Tests
{
    public sealed class BuildingDatabaseTests
    {
        [Test]
        public void OpenAsync_RejectsPathOutsideDatasetRoot()
        {
            Assert.Throws<InvalidDataException>(() => BuildingDatabase.OpenAsync("../citymodel.sqlite", path => new FakeStore(), CancellationToken.None));
        }

        [Test]
        public void FixedStoreOperationsForwardOnlyBuildingId()
        {
            using (var database = BuildingDatabase.OpenAsync("citymodel.sqlite", path => new FakeStore(), CancellationToken.None).GetAwaiter().GetResult())
            {
                var building = database.FindBuildingAsync("bldg-1", CancellationToken.None).GetAwaiter().GetResult();
                var attributes = database.FindAttributesAsync("bldg-1", CancellationToken.None).GetAwaiter().GetResult();
                Assert.That(building.BuildingId, Is.EqualTo("bldg-1"));
                Assert.That(attributes[0].Key, Is.EqualTo("bldg:usage"));
            }
        }

        private sealed class FakeStore : IReadOnlyBuildingStore
        {
            public BuildingRecord FindBuilding(string buildingId) { return new BuildingRecord { BuildingId = buildingId, CanonicalBuildingId = "dataset::" + buildingId, TileId = "tile" }; }
            public IReadOnlyList<BuildingAttribute> FindAttributes(string buildingId) { return new[] { new BuildingAttribute { Key = "bldg:usage", Value = "residential" } }; }
            public void Dispose() { }
        }
    }
}
