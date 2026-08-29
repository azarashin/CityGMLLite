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

        [Test]
        public void FixedFeatureStoreOperationsReturnGenericIdentityAndAttributes()
        {
            using (var database = BuildingDatabase.OpenAsync("citymodel.sqlite", path => new FeatureFakeStore(), CancellationToken.None).GetAwaiter().GetResult())
            {
                var feature = database.FindFeatureAsync("terrain-1", CancellationToken.None).GetAwaiter().GetResult();
                var attributes = database.FindFeatureAttributesAsync("terrain-1", CancellationToken.None).GetAwaiter().GetResult();
                Assert.That(feature.FeatureType, Is.EqualTo("terrain"));
                Assert.That(attributes[0].Key, Is.EqualTo("dem:lod"));
            }
        }

        private sealed class FakeStore : IReadOnlyBuildingStore
        {
            public BuildingRecord FindBuilding(string buildingId) { return new BuildingRecord { BuildingId = buildingId, CanonicalBuildingId = "dataset::" + buildingId, TileId = "tile" }; }
            public IReadOnlyList<BuildingAttribute> FindAttributes(string buildingId) { return new[] { new BuildingAttribute { Key = "bldg:usage", Value = "residential" } }; }
            public void Dispose() { }
        }

        private sealed class FeatureFakeStore : IReadOnlyBuildingStore, IReadOnlyFeatureStore
        {
            public BuildingRecord FindBuilding(string buildingId) { return null; }
            public IReadOnlyList<BuildingAttribute> FindAttributes(string buildingId) { return new BuildingAttribute[0]; }
            public FeatureRecord FindFeature(string featureId) { return new FeatureRecord { FeatureId = featureId, CanonicalFeatureId = "dataset::" + featureId, FeatureType = "terrain" }; }
            public IReadOnlyList<FeatureAttribute> FindFeatureAttributes(string featureId) { return new[] { new FeatureAttribute { Key = "dem:lod", Value = "1" } }; }
            public void Dispose() { }
        }
    }
}
