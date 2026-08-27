using System;
using System.IO;
using System.Text;
using UnityEngine;
using UnityEngine.Rendering;

namespace CityModel.Loading
{
    /// <summary>Decodes the constrained GLB 2.0 layout emitted by CityGMLLite's converter.</summary>
    public static class CityModelGlbDecoder
    {
        private const uint GlbMagic = 0x46546C67;
        private const uint JsonChunkType = 0x4E4F534A;
        private const uint BinChunkType = 0x004E4942;

        [Serializable] private sealed class GlbRoot { public GlbBufferView[] bufferViews; public GlbAccessor[] accessors; public GlbMesh[] meshes; }
        [Serializable] private sealed class GlbBufferView { public int buffer; public int byteOffset; public int byteLength; public int byteStride; }
        [Serializable] private sealed class GlbAccessor { public int bufferView; public int byteOffset; public int componentType; public int count; public string type; }
        [Serializable] private sealed class GlbMesh { public GlbPrimitive[] primitives; }
        [Serializable] private sealed class GlbPrimitive { public GlbAttributes attributes; public int indices; public int mode; }
        [Serializable] private sealed class GlbAttributes { public int POSITION = -1; public int NORMAL = -1; public int _FEATURE_ID_0 = -1; }

        public static Mesh Decode(byte[] glbBytes, string meshName)
        {
            if (glbBytes == null || glbBytes.Length < 20)
                throw new InvalidDataException("GLB is too short.");
            var offset = 0;
            if (ReadUInt32(glbBytes, ref offset) != GlbMagic || ReadUInt32(glbBytes, ref offset) != 2)
                throw new InvalidDataException("Expected a GLB 2.0 file.");
            var declaredLength = ReadUInt32(glbBytes, ref offset);
            if (declaredLength != glbBytes.Length)
                throw new InvalidDataException("GLB length does not match its header.");
            var json = ReadChunk(glbBytes, ref offset, JsonChunkType);
            var binary = ReadChunk(glbBytes, ref offset, BinChunkType);
            if (offset != glbBytes.Length)
                throw new InvalidDataException("GLB has unexpected trailing data.");

            var root = JsonUtility.FromJson<GlbRoot>(Encoding.UTF8.GetString(json).TrimEnd('\0', ' ', '\t', '\r', '\n'));
            if (root == null || root.meshes == null || root.meshes.Length != 1 || root.meshes[0].primitives == null || root.meshes[0].primitives.Length != 1)
                throw new InvalidDataException("Expected exactly one mesh primitive in the GLB.");
            var primitive = root.meshes[0].primitives[0];
            if (primitive.mode != 4 || primitive.attributes == null)
                throw new InvalidDataException("Expected triangle primitive attributes.");

            var vertices = ReadVector3Accessor(root, binary, primitive.attributes.POSITION, "POSITION");
            var normals = ReadVector3Accessor(root, binary, primitive.attributes.NORMAL, "NORMAL");
            if (vertices.Length != normals.Length)
                throw new InvalidDataException("POSITION and NORMAL counts differ.");
            var indices = ReadIndexAccessor(root, binary, primitive.indices, vertices.Length);
            ValidateFeatureIds(root, binary, primitive.attributes._FEATURE_ID_0, vertices.Length);

            var mesh = new Mesh { name = meshName, indexFormat = IndexFormat.UInt32 };
            mesh.vertices = vertices;
            mesh.normals = normals;
            mesh.triangles = indices;
            mesh.RecalculateBounds();
            return mesh;
        }

        private static Vector3[] ReadVector3Accessor(GlbRoot root, byte[] binary, int accessorIndex, string name)
        {
            var accessor = GetAccessor(root, accessorIndex, name);
            if (accessor.componentType != 5126 || accessor.type != "VEC3")
                throw new InvalidDataException(name + " must be FLOAT VEC3.");
            var view = GetBufferView(root, accessor.bufferView, name);
            var stride = view.byteStride == 0 ? 12 : view.byteStride;
            if (stride < 12) throw new InvalidDataException(name + " stride is too small.");
            var values = new Vector3[accessor.count];
            for (var i = 0; i < values.Length; i++)
            {
                var entry = GetEntryOffset(view, accessor, binary, stride, i, 12, name);
                values[i] = new Vector3(ReadSingle(binary, entry), ReadSingle(binary, entry + 4), ReadSingle(binary, entry + 8));
            }
            return values;
        }

        private static int[] ReadIndexAccessor(GlbRoot root, byte[] binary, int accessorIndex, int vertexCount)
        {
            var accessor = GetAccessor(root, accessorIndex, "indices");
            if (accessor.componentType != 5125 || accessor.type != "SCALAR" || accessor.count % 3 != 0)
                throw new InvalidDataException("indices must be UNSIGNED_INT triangle scalars.");
            var view = GetBufferView(root, accessor.bufferView, "indices");
            var stride = view.byteStride == 0 ? 4 : view.byteStride;
            if (stride < 4) throw new InvalidDataException("indices stride is too small.");
            var values = new int[accessor.count];
            for (var i = 0; i < values.Length; i++)
            {
                var raw = ReadUInt32(binary, GetEntryOffset(view, accessor, binary, stride, i, 4, "indices"));
                if (raw >= vertexCount) throw new InvalidDataException("indices reference a vertex outside POSITION.");
                values[i] = (int)raw;
            }
            return values;
        }

        private static void ValidateFeatureIds(GlbRoot root, byte[] binary, int accessorIndex, int vertexCount)
        {
            if (accessorIndex < 0) return;
            var accessor = GetAccessor(root, accessorIndex, "_FEATURE_ID_0");
            if (accessor.componentType != 5123 || accessor.type != "SCALAR" || accessor.count != vertexCount)
                throw new InvalidDataException("_FEATURE_ID_0 must be UNSIGNED_SHORT with one value per vertex.");
            var view = GetBufferView(root, accessor.bufferView, "_FEATURE_ID_0");
            var stride = view.byteStride == 0 ? 2 : view.byteStride;
            if (stride < 2) throw new InvalidDataException("_FEATURE_ID_0 stride is too small.");
            for (var i = 0; i < accessor.count; i++) GetEntryOffset(view, accessor, binary, stride, i, 2, "_FEATURE_ID_0");
        }

        private static GlbAccessor GetAccessor(GlbRoot root, int index, string name)
        {
            if (root.accessors == null || index < 0 || index >= root.accessors.Length) throw new InvalidDataException(name + " accessor is missing.");
            var accessor = root.accessors[index];
            if (accessor.count < 0) throw new InvalidDataException(name + " count is invalid.");
            return accessor;
        }

        private static GlbBufferView GetBufferView(GlbRoot root, int index, string name)
        {
            if (root.bufferViews == null || index < 0 || index >= root.bufferViews.Length) throw new InvalidDataException(name + " bufferView is missing.");
            var view = root.bufferViews[index];
            if (view.buffer != 0 || view.byteOffset < 0 || view.byteLength < 0 || view.byteStride < 0) throw new InvalidDataException(name + " bufferView is invalid.");
            return view;
        }

        private static int GetEntryOffset(GlbBufferView view, GlbAccessor accessor, byte[] binary, int stride, int index, int size, string name)
        {
            var start = checked(view.byteOffset + accessor.byteOffset + checked(index * stride));
            var end = checked(start + size);
            var viewEnd = checked(view.byteOffset + view.byteLength);
            if (start < view.byteOffset || end > viewEnd || end > binary.Length) throw new InvalidDataException(name + " data exceeds its bufferView.");
            return start;
        }

        private static byte[] ReadChunk(byte[] bytes, ref int offset, uint expectedType)
        {
            if (offset > bytes.Length - 8) throw new InvalidDataException("GLB chunk header is missing.");
            var length = ReadUInt32(bytes, ref offset);
            if (ReadUInt32(bytes, ref offset) != expectedType || length > bytes.Length - offset) throw new InvalidDataException("GLB chunk is invalid.");
            var chunk = new byte[(int)length];
            Buffer.BlockCopy(bytes, offset, chunk, 0, (int)length);
            offset += (int)length;
            return chunk;
        }

        private static uint ReadUInt32(byte[] bytes, ref int offset) { var value = ReadUInt32(bytes, offset); offset += 4; return value; }
        private static uint ReadUInt32(byte[] bytes, int offset)
        {
            if (offset < 0 || offset > bytes.Length - 4) throw new InvalidDataException("GLB data is truncated.");
            return (uint)(bytes[offset] | bytes[offset + 1] << 8 | bytes[offset + 2] << 16 | bytes[offset + 3] << 24);
        }
        private static float ReadSingle(byte[] bytes, int offset)
        {
            if (offset < 0 || offset > bytes.Length - 4) throw new InvalidDataException("GLB data is truncated.");
            return BitConverter.ToSingle(bytes, offset);
        }
    }
}
