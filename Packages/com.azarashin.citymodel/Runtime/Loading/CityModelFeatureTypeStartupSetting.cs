using System;
using UnityEngine;

namespace CityModel.Loading
{
    /// <summary>Known CityGMLLite feature-type names used by generated artifact indexes.</summary>
    public static class CityModelFeatureTypes
    {
        public const string Building = "building";
        public const string Terrain = "terrain";
        public const string Water = "water";
        public const string Transportation = "transportation";
    }

    /// <summary>Inspector-visible startup policy for one generated feature type.</summary>
    [Serializable]
    public sealed class CityModelFeatureTypeStartupSetting
    {
        [Tooltip("The generated featureType to load, such as building, terrain, water, or transportation.")]
        public string featureType = CityModelFeatureTypes.Building;

        [Tooltip("When disabled, this type's metadata, GLB, textures, and attributes are not opened or created at startup.")]
        public bool loadOnStartup = true;

        [Tooltip("When enabled, loaded renderers for this type start visible. This is ignored when Load On Startup is disabled.")]
        public bool initiallyVisible = true;
    }

    /// <summary>Resolves serialized startup policies without performing dataset I/O.</summary>
    public static class CityModelFeatureTypeStartupSettings
    {
        /// <summary>
        /// Resolves one type's policy. An empty configuration retains the pre-type-index
        /// behaviour for legacy scenes: buildings load and are visible; other types do not load.
        /// </summary>
        public static bool TryResolve(
            CityModelFeatureTypeStartupSetting[] settings,
            string featureType,
            out bool loadOnStartup,
            out bool initiallyVisible)
        {
            if (string.IsNullOrWhiteSpace(featureType))
            {
                loadOnStartup = false;
                initiallyVisible = false;
                return false;
            }

            if (settings != null)
            {
                foreach (var setting in settings)
                {
                    if (setting == null || string.IsNullOrWhiteSpace(setting.featureType)) continue;
                    if (!string.Equals(setting.featureType.Trim(), featureType, StringComparison.OrdinalIgnoreCase)) continue;
                    loadOnStartup = setting.loadOnStartup;
                    initiallyVisible = setting.loadOnStartup && setting.initiallyVisible;
                    return true;
                }
            }

            if (settings == null || settings.Length == 0)
            {
                loadOnStartup = string.Equals(featureType, CityModelFeatureTypes.Building, StringComparison.OrdinalIgnoreCase);
                initiallyVisible = loadOnStartup;
                return loadOnStartup;
            }

            loadOnStartup = false;
            initiallyVisible = false;
            return false;
        }
    }
}
