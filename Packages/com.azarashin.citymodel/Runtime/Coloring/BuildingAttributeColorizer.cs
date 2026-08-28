using System;
using System.Collections.Generic;
using System.Globalization;
using CityModel.Database;
using UnityEngine;

namespace CityModel.Coloring
{
    public enum BuildingAttributeColorMode { Usage, MeasuredHeight }

    /// <summary>Deterministic color rules used by the Quick Start attribute visualization.</summary>
    public static class BuildingAttributeColorizer
    {
        public static readonly Color32 MissingAttributeColor = new Color32(142, 151, 163, 255);
        private static readonly Color32[] UsagePalette =
        {
            new Color32(66, 133, 244, 255), new Color32(52, 168, 83, 255), new Color32(251, 188, 5, 255),
            new Color32(234, 67, 53, 255), new Color32(171, 71, 188, 255), new Color32(0, 172, 193, 255)
        };

        public static Color32 ColorFor(BuildingAttributeColorMode mode, IReadOnlyList<BuildingAttribute> attributes, float minimumHeight, float maximumHeight)
        {
            if (attributes == null) return MissingAttributeColor;
            if (mode == BuildingAttributeColorMode.Usage)
            {
                var usage = FindValue(attributes, "usage");
                return string.IsNullOrWhiteSpace(usage) ? MissingAttributeColor : UsagePalette[StableIndex(usage, UsagePalette.Length)];
            }

            var value = FindHeight(attributes);
            if (!value.HasValue) return MissingAttributeColor;
            var normalized = maximumHeight > minimumHeight ? Mathf.InverseLerp(minimumHeight, maximumHeight, value.Value) : 0.5f;
            return Color.Lerp(new Color32(44, 123, 182, 255), new Color32(215, 48, 39, 255), normalized);
        }

        public static float? FindHeight(IReadOnlyList<BuildingAttribute> attributes)
        {
            var value = FindValue(attributes, "measuredHeight");
            if (float.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var height) && !float.IsNaN(height) && !float.IsInfinity(height)) return height;
            return null;
        }

        private static string FindValue(IReadOnlyList<BuildingAttribute> attributes, string key)
        {
            for (var index = 0; index < attributes.Count; index++)
            {
                var attribute = attributes[index];
                if (attribute != null && (attribute.Key == key || attribute.Key.EndsWith(":" + key, StringComparison.Ordinal))) return attribute.Value;
            }
            return null;
        }

        private static int StableIndex(string value, int count)
        {
            unchecked
            {
                uint hash = 2166136261;
                for (var index = 0; index < value.Length; index++) { hash ^= value[index]; hash *= 16777619; }
                return (int)(hash % (uint)count);
            }
        }
    }
}
