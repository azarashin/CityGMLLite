using CityModel.Coloring;
using CityModel.Database;
using NUnit.Framework;
using UnityEngine;

namespace CityModel.Tests
{
    public sealed class BuildingAttributeColorizerTests
    {
        [Test]
        public void UsageColor_IsStableForSameCategory()
        {
            var attributes = new[] { new BuildingAttribute { Key = "bldg:usage", Value = "residential" } };
            var first = BuildingAttributeColorizer.ColorFor(BuildingAttributeColorMode.Usage, attributes, 0f, 0f);
            var second = BuildingAttributeColorizer.ColorFor(BuildingAttributeColorMode.Usage, attributes, 0f, 0f);
            Assert.That(first, Is.EqualTo(second));
            Assert.That(first, Is.Not.EqualTo(BuildingAttributeColorizer.MissingAttributeColor));
        }

        [Test]
        public void MeasuredHeightColor_UsesContinuousRangeAndDefaultForMissingValue()
        {
            var low = new[] { new BuildingAttribute { Key = "measuredHeight", Value = "10.5" } };
            var high = new[] { new BuildingAttribute { Key = "bldg:measuredHeight", Value = "80.0" } };
            var lowColor = BuildingAttributeColorizer.ColorFor(BuildingAttributeColorMode.MeasuredHeight, low, 10.5f, 80f);
            var highColor = BuildingAttributeColorizer.ColorFor(BuildingAttributeColorMode.MeasuredHeight, high, 10.5f, 80f);
            Assert.That(lowColor, Is.Not.EqualTo(highColor));
            Assert.That(BuildingAttributeColorizer.ColorFor(BuildingAttributeColorMode.MeasuredHeight, new BuildingAttribute[0], 0f, 1f), Is.EqualTo(BuildingAttributeColorizer.MissingAttributeColor));
        }
    }
}
