using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace CityModel.Database
{
    /// <summary>
    /// Fixed-query, read-only SQLite store for converter artifacts. This intentionally exposes
    /// no SQL execution surface and never enables SQLite extensions.
    /// </summary>
    internal sealed class SqliteReadOnlyBuildingStore : IReadOnlyBuildingStore
    {
        private const int SqliteOk = 0;
        private const int SqliteRow = 100;
        private const int SqliteDone = 101;
        private const int SqliteNull = 5;
        private const int SqliteOpenReadOnly = 0x00000001;
        private const int SqliteOpenNoMutex = 0x00008000;
        private const int SqliteOpenPrivateCache = 0x00040000;
        private static readonly IntPtr SqliteTransient = new IntPtr(-1);
        private const string FindBuildingSql = "SELECT building_id, canonical_building_id, tile_id FROM buildings WHERE building_id = ?1 LIMIT 1";
        private const string FindAttributesSql = "SELECT attribute_key, value_type, value_text, value_integer, value_real, value_boolean, value_datetime, uom, code_space FROM building_attributes WHERE building_id = ?1 ORDER BY attribute_key, ordinal";

        private readonly object _sync = new object();
        private IntPtr _connection;
        private bool _disposed;

        private SqliteReadOnlyBuildingStore(IntPtr connection) { _connection = connection; }

        public static SqliteReadOnlyBuildingStore Open(string databasePath)
        {
            EnsureWindowsX64();
            IntPtr connection;
            var result = sqlite3_open_v2(databasePath, out connection, SqliteOpenReadOnly | SqliteOpenNoMutex | SqliteOpenPrivateCache, IntPtr.Zero);
            if (result != SqliteOk)
            {
                var message = connection == IntPtr.Zero ? "SQLite could not open the database." : GetError(connection);
                if (connection != IntPtr.Zero) sqlite3_close(connection);
                throw new InvalidDataException(message);
            }
            return new SqliteReadOnlyBuildingStore(connection);
        }

        public BuildingRecord FindBuilding(string buildingId)
        {
            if (string.IsNullOrWhiteSpace(buildingId)) return null;
            lock (_sync)
            {
                ThrowIfDisposed();
                IntPtr statement = IntPtr.Zero;
                try
                {
                    PrepareAndBind(FindBuildingSql, buildingId, out statement);
                    var result = sqlite3_step(statement);
                    if (result == SqliteDone) return null;
                    ThrowIfNotRow(result);
                    return new BuildingRecord { BuildingId = GetText(statement, 0), CanonicalBuildingId = GetText(statement, 1), TileId = GetText(statement, 2) };
                }
                finally { if (statement != IntPtr.Zero) sqlite3_finalize(statement); }
            }
        }

        public IReadOnlyList<BuildingAttribute> FindAttributes(string buildingId)
        {
            var attributes = new List<BuildingAttribute>();
            if (string.IsNullOrWhiteSpace(buildingId)) return attributes;
            lock (_sync)
            {
                ThrowIfDisposed();
                IntPtr statement = IntPtr.Zero;
                try
                {
                    PrepareAndBind(FindAttributesSql, buildingId, out statement);
                    for (;;)
                    {
                        var result = sqlite3_step(statement);
                        if (result == SqliteDone) return attributes;
                        ThrowIfNotRow(result);
                        attributes.Add(new BuildingAttribute
                        {
                            Key = GetText(statement, 0),
                            Value = ReadAttributeValue(statement),
                            Unit = GetText(statement, 7),
                            CodeSpace = GetText(statement, 8)
                        });
                    }
                }
                finally { if (statement != IntPtr.Zero) sqlite3_finalize(statement); }
            }
        }

        public void Dispose()
        {
            lock (_sync)
            {
                if (_disposed) return;
                _disposed = true;
                if (_connection != IntPtr.Zero)
                {
                    sqlite3_close(_connection);
                    _connection = IntPtr.Zero;
                }
            }
        }

        internal DatasetDatabaseMetadata ReadMetadata()
        {
            const string sql = "SELECT schema_version, generation_id, database_sha256 FROM dataset_metadata LIMIT 1";
            lock (_sync)
            {
                ThrowIfDisposed();
                IntPtr statement = IntPtr.Zero;
                try
                {
                    Prepare(sql, out statement);
                    var result = sqlite3_step(statement);
                    if (result == SqliteDone) throw new InvalidDataException("SQLite dataset metadata is missing.");
                    ThrowIfNotRow(result);
                    return new DatasetDatabaseMetadata(GetText(statement, 0), GetText(statement, 1), GetText(statement, 2));
                }
                finally { if (statement != IntPtr.Zero) sqlite3_finalize(statement); }
            }
        }

        /// <summary>
        /// The building queries are compatible with v1 and v2 databases; callers
        /// must still reject future versions before relying on the v1 tables.
        /// </summary>
        internal int ReadUserVersion()
        {
            const string sql = "PRAGMA user_version";
            lock (_sync)
            {
                ThrowIfDisposed();
                IntPtr statement = IntPtr.Zero;
                try
                {
                    Prepare(sql, out statement);
                    ThrowIfNotRow(sqlite3_step(statement));
                    return checked((int)sqlite3_column_int64(statement, 0));
                }
                finally { if (statement != IntPtr.Zero) sqlite3_finalize(statement); }
            }
        }

        private void PrepareAndBind(string sql, string value, out IntPtr statement)
        {
            Prepare(sql, out statement);
            var bytes = Encoding.UTF8.GetBytes(value);
            var result = sqlite3_bind_text(statement, 1, bytes, bytes.Length, SqliteTransient);
            if (result != SqliteOk) throw new InvalidDataException(GetError(_connection));
        }

        private void Prepare(string sql, out IntPtr statement)
        {
            var result = sqlite3_prepare_v2(_connection, sql, -1, out statement, IntPtr.Zero);
            if (result != SqliteOk) throw new InvalidDataException(GetError(_connection));
        }

        private void ThrowIfNotRow(int result)
        {
            if (result != SqliteRow) throw new InvalidDataException(GetError(_connection));
        }

        private static string ReadAttributeValue(IntPtr statement)
        {
            var type = GetText(statement, 1);
            if (type == "real" && sqlite3_column_type(statement, 4) != SqliteNull)
                return sqlite3_column_double(statement, 4).ToString("R", CultureInfo.InvariantCulture);
            if (type == "integer" && sqlite3_column_type(statement, 3) != SqliteNull)
                return sqlite3_column_int64(statement, 3).ToString(CultureInfo.InvariantCulture);
            if (type == "boolean" && sqlite3_column_type(statement, 5) != SqliteNull)
                return sqlite3_column_int64(statement, 5) == 0 ? "false" : "true";
            if (type == "datetime") return GetText(statement, 6);
            return GetText(statement, 2);
        }

        private static string GetText(IntPtr statement, int index)
        {
            if (sqlite3_column_type(statement, index) == SqliteNull) return null;
            var pointer = sqlite3_column_text(statement, index);
            var count = sqlite3_column_bytes(statement, index);
            if (pointer == IntPtr.Zero || count <= 0) return string.Empty;
            var bytes = new byte[count];
            Marshal.Copy(pointer, bytes, 0, count);
            return Encoding.UTF8.GetString(bytes);
        }

        private static string GetError(IntPtr connection)
        {
            var pointer = sqlite3_errmsg(connection);
            return pointer == IntPtr.Zero ? "SQLite operation failed." : Marshal.PtrToStringAnsi(pointer);
        }

        private static void EnsureWindowsX64()
        {
#if UNITY_EDITOR_WIN || UNITY_STANDALONE_WIN
            if (IntPtr.Size != 8) throw new PlatformNotSupportedException("CityModel SQLite access requires a Windows x64 player.");
#else
            throw new PlatformNotSupportedException("CityModel SQLite access is currently supported only on Windows x64.");
#endif
        }

        private void ThrowIfDisposed()
        {
            if (_disposed) throw new ObjectDisposedException(nameof(SqliteReadOnlyBuildingStore));
        }

        // Windows ships winsqlite3.dll, avoiding an application-controlled native DLL search path.
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
        private static extern int sqlite3_open_v2([MarshalAs(UnmanagedType.LPUTF8Str)] string filename, out IntPtr database, int flags, IntPtr vfs);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern int sqlite3_close(IntPtr database);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern IntPtr sqlite3_errmsg(IntPtr database);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern int sqlite3_prepare_v2(IntPtr database, [MarshalAs(UnmanagedType.LPUTF8Str)] string sql, int bytes, out IntPtr statement, IntPtr tail);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern int sqlite3_finalize(IntPtr statement);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern int sqlite3_step(IntPtr statement);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern int sqlite3_bind_text(IntPtr statement, int index, byte[] value, int byteCount, IntPtr destructor);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern IntPtr sqlite3_column_text(IntPtr statement, int index);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern int sqlite3_column_bytes(IntPtr statement, int index);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern int sqlite3_column_type(IntPtr statement, int index);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern long sqlite3_column_int64(IntPtr statement, int index);
        [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)] private static extern double sqlite3_column_double(IntPtr statement, int index);
    }

    internal sealed class DatasetDatabaseMetadata
    {
        public DatasetDatabaseMetadata(string schemaVersion, string generationId, string databaseSha256)
        {
            SchemaVersion = schemaVersion;
            GenerationId = generationId;
            DatabaseSha256 = databaseSha256;
        }
        public string SchemaVersion { get; }
        public string GenerationId { get; }
        public string DatabaseSha256 { get; }
    }
}
