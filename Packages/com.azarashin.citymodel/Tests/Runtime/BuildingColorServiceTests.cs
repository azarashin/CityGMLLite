using CityModel.Coloring;
using NUnit.Framework;
using UnityEngine;

namespace CityModel.Tests
{
    public sealed class BuildingColorServiceTests
    {
        [Test]
        public void ApplyToMaterial_BindsTheRegisteredTileColorTable()
        {
            var shader = Shader.Find("CityModel/Feature Colors");
            Assert.That(shader, Is.Not.Null);
            var material = new Material(shader);
            var colors = new BuildingColorService(BuildingAttributeColorizer.MissingAttributeColor);
            try
            {
                colors.RegisterTile("tile-a", new[] { "building-a", "building-b" });
                colors.SetColor("building-a", new Color32(1, 2, 3, 255));
                colors.ApplyToMaterial("tile-a", material);

                Assert.That(material.GetInt("_CityModelFeatureColorCount"), Is.EqualTo(2));
                Assert.That(material.GetColor("_CityModelDefaultColor"), Is.EqualTo((Color)BuildingAttributeColorizer.MissingAttributeColor));
            }
            finally
            {
                colors.Dispose();
                Object.DestroyImmediate(material);
            }
        }

        [Test]
        public void RegisterTile_ReplacesAndRebindsOnlyThatTilesTable()
        {
            var shader = Shader.Find("CityModel/Feature Colors");
            Assert.That(shader, Is.Not.Null);
            var firstMaterial = new Material(shader);
            var replacementMaterial = new Material(shader);
            var colors = new BuildingColorService(BuildingAttributeColorizer.MissingAttributeColor);
            try
            {
                colors.RegisterTile("tile-a", new[] { "building-a" });
                colors.ApplyToMaterial("tile-a", firstMaterial);
                colors.RegisterTile("tile-a", new[] { "building-a", "building-b" });
                colors.SetColor("building-b", new Color32(5, 6, 7, 255));
                colors.ApplyToMaterial("tile-a", replacementMaterial);

                Assert.That(firstMaterial.GetInt("_CityModelFeatureColorCount"), Is.EqualTo(1));
                Assert.That(replacementMaterial.GetInt("_CityModelFeatureColorCount"), Is.EqualTo(2));
            }
            finally
            {
                colors.Dispose();
                Object.DestroyImmediate(firstMaterial);
                Object.DestroyImmediate(replacementMaterial);
            }
        }
    }
}
