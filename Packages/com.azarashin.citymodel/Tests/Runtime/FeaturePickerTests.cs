using CityModel.Loading;
using CityModel.Picking;
using NUnit.Framework;

namespace CityModel.Tests
{
    public sealed class FeaturePickerTests
    {
        [Test]
        public void TryResolveFeature_ReturnsTypeAndPersistentIdForLocalId()
        {
            var features = new[]
            {
                new GenericTileFeature { localFeatureId = 3, featureType = CityModelFeatureTypes.Terrain, featureId = "terrain-3" },
            };

            Assert.That(FeaturePicker.TryResolveFeature(features, 3, out var feature), Is.True);
            Assert.That(feature.featureType, Is.EqualTo(CityModelFeatureTypes.Terrain));
            Assert.That(feature.featureId, Is.EqualTo("terrain-3"));
        }

        [Test]
        public void TryResolveFeature_RejectsUnknownOrIncompleteMapping()
        {
            var features = new[]
            {
                new GenericTileFeature { localFeatureId = 1, featureType = CityModelFeatureTypes.Water },
            };

            Assert.That(FeaturePicker.TryResolveFeature(features, 1, out _), Is.False);
            Assert.That(FeaturePicker.TryResolveFeature(features, 2, out _), Is.False);
        }
    }
}
