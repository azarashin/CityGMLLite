using CityModel.Loading;
using NUnit.Framework;

namespace CityModel.Tests
{
    public sealed class CityModelFeatureTypeStartupSettingsTests
    {
        [Test]
        public void TryResolve_LoadDisabled_MakesInitialVisibilityIneffective()
        {
            var settings = new[]
            {
                new CityModelFeatureTypeStartupSetting
                {
                    featureType = CityModelFeatureTypes.Terrain,
                    loadOnStartup = false,
                    initiallyVisible = true,
                },
            };

            Assert.That(CityModelFeatureTypeStartupSettings.TryResolve(settings, CityModelFeatureTypes.Terrain, out var load, out var visible), Is.True);
            Assert.That(load, Is.False);
            Assert.That(visible, Is.False);
        }

        [Test]
        public void TryResolve_LoadedButHidden_PreservesBothStates()
        {
            var settings = new[]
            {
                new CityModelFeatureTypeStartupSetting
                {
                    featureType = CityModelFeatureTypes.Terrain,
                    loadOnStartup = true,
                    initiallyVisible = false,
                },
            };

            Assert.That(CityModelFeatureTypeStartupSettings.TryResolve(settings, CityModelFeatureTypes.Terrain, out var load, out var visible), Is.True);
            Assert.That(load, Is.True);
            Assert.That(visible, Is.False);
        }

        [Test]
        public void TryResolve_EmptyLegacyConfiguration_LoadsOnlyBuildings()
        {
            Assert.That(CityModelFeatureTypeStartupSettings.TryResolve(new CityModelFeatureTypeStartupSetting[0], CityModelFeatureTypes.Building, out var buildingLoad, out var buildingVisible), Is.True);
            Assert.That(buildingLoad, Is.True);
            Assert.That(buildingVisible, Is.True);

            Assert.That(CityModelFeatureTypeStartupSettings.TryResolve(new CityModelFeatureTypeStartupSetting[0], CityModelFeatureTypes.Terrain, out var terrainLoad, out var terrainVisible), Is.False);
            Assert.That(terrainLoad, Is.False);
            Assert.That(terrainVisible, Is.False);
        }
    }
}
