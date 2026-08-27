namespace CityModel.Versioning
{
    /// <summary>
    /// Version values shared with <c>contracts/version.json</c> and converter output.
    /// Issue #1 will formalize generation and compatibility validation for these values.
    /// </summary>
    public static class CityModelContractVersion
    {
        public const string SchemaVersion = "1.0.0";
        public const string GeneratorVersion = "0.1.0-dev";
        public const int DatabaseUserVersion = 1;
    }
}
