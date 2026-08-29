using System;
using System.IO;
using System.Text;
using UnityEngine;
using UnityEngine.Rendering;

namespace CityModel.Loading
{
    /// <summary>Mesh data plus the converter's tile-local Feature ID for every vertex.</summary>
    public sealed class DecodedTileMesh
    {
        public DecodedTileMesh(Mesh mesh, ushort[] featureIds, ushort[] triangleFeatureIds, Texture2D[] textures = null)
        {
            Mesh = mesh;
            FeatureIds = featureIds;
            TriangleFeatureIds = triangleFeatureIds;
            Textures = textures ?? Array.Empty<Texture2D>();
        }
        public Mesh Mesh { get; }
        public ushort[] FeatureIds { get; }
        /// <summary>One local feature ID for each collider triangle, in submesh order.</summary>
        public ushort[] TriangleFeatureIds { get; }
        /// <summary>One texture per mesh submesh for embedded, textured terrain content.</summary>
        public Texture2D[] Textures { get; }
        public bool HasEmbeddedTextures => Textures.Length > 0;
    }

    /// <summary>Decodes the constrained GLB 2.0 layout emitted by CityGMLLite's converter.</summary>
    public static class CityModelGlbDecoder
    {
        private const uint GlbMagic = 0x46546C67;
        private const uint JsonChunkType = 0x4E4F534A;
        private const uint BinChunkType = 0x004E4942;
        private const int MaxGlbBytes = 128 * 1024 * 1024;
        private const int MaxImageBytes = 64 * 1024 * 1024;
        private const int MaxImageDimension = 16384;

        [Serializable] private sealed class GlbRoot { public GlbBufferView[] bufferViews; public GlbAccessor[] accessors; public GlbMesh[] meshes; public GlbImage[] images; public GlbTexture[] textures; public GlbMaterial[] materials; }
        [Serializable] private sealed class GlbBufferView { public int buffer; public int byteOffset; public int byteLength; public int byteStride; }
        [Serializable] private sealed class GlbAccessor { public int bufferView; public int byteOffset; public int componentType; public int count; public string type; }
        [Serializable] private sealed class GlbMesh { public GlbPrimitive[] primitives; }
        [Serializable] private sealed class GlbPrimitive { public GlbAttributes attributes; public int indices = -1; public int material = -1; public int mode = 4; }
        [Serializable] private sealed class GlbAttributes { public int POSITION = -1; public int NORMAL = -1; public int TEXCOORD_0 = -1; public int _FEATURE_ID_0 = -1; }
        [Serializable] private sealed class GlbImage { public int bufferView = -1; public string mimeType; public string uri; }
        [Serializable] private sealed class GlbTexture { public int source = -1; }
        [Serializable] private sealed class GlbMaterial { public GlbPbrMetallicRoughness pbrMetallicRoughness; }
        [Serializable] private sealed class GlbPbrMetallicRoughness { public GlbTextureInfo baseColorTexture; }
        [Serializable] private sealed class GlbTextureInfo { public int index = -1; }

        public static Mesh Decode(byte[] glbBytes, string meshName)
        {
            return DecodeWithFeatureIds(glbBytes, meshName).Mesh;
        }

        public static DecodedTileMesh DecodeWithFeatureIds(byte[] glbBytes, string meshName)
        {
            if (glbBytes == null || glbBytes.Length < 20 || glbBytes.Length > MaxGlbBytes)
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
            if (root == null || root.meshes == null || root.meshes.Length != 1 || root.meshes[0].primitives == null || root.meshes[0].primitives.Length == 0)
                throw new InvalidDataException("Expected one mesh with at least one primitive in the GLB.");
            var primitives = root.meshes[0].primitives;
            var first = primitives[0];
            if (first.mode != 4 || first.attributes == null)
                throw new InvalidDataException("Expected triangle primitive attributes.");

            var vertices = ReadVector3Accessor(root, binary, first.attributes.POSITION, "POSITION");
            var normals = ReadVector3Accessor(root, binary, first.attributes.NORMAL, "NORMAL");
            var featureIds = ReadFeatureIds(root, binary, first.attributes._FEATURE_ID_0, vertices.Length);
            var texturedTerrain = first.attributes.TEXCOORD_0 >= 0;
            Vector2[] uvs = texturedTerrain ? ReadVector2Accessor(root, binary, first.attributes.TEXCOORD_0, "TEXCOORD_0") : null;
            if (vertices.Length != normals.Length || (uvs != null && vertices.Length != uvs.Length))
                throw new InvalidDataException("POSITION attribute counts do not match NORMAL or TEXCOORD_0.");
            var indices = new int[primitives.Length][];
            for (var primitiveIndex = 0; primitiveIndex < primitives.Length; primitiveIndex++)
            {
                var primitive = primitives[primitiveIndex];
                if (primitive.mode != 4 || primitive.attributes == null || primitive.attributes.POSITION != first.attributes.POSITION || primitive.attributes.NORMAL != first.attributes.NORMAL || primitive.attributes._FEATURE_ID_0 != first.attributes._FEATURE_ID_0 || primitive.attributes.TEXCOORD_0 != first.attributes.TEXCOORD_0)
                    throw new InvalidDataException("Terrain primitives must share the same vertex attributes.");
                if (texturedTerrain != (primitive.attributes.TEXCOORD_0 >= 0))
                    throw new InvalidDataException("GLB must not mix textured and untextured primitives.");
                indices[primitiveIndex] = ReadIndexAccessor(root, binary, primitive.indices, vertices.Length);
            }
            var textures = texturedTerrain ? ReadPrimitiveTextures(root, binary, primitives) : Array.Empty<Texture2D>();

            var mesh = new Mesh { name = meshName, indexFormat = IndexFormat.UInt32 };
            mesh.vertices = vertices;
            mesh.normals = normals;
            mesh.subMeshCount = indices.Length;
            for (var primitiveIndex = 0; primitiveIndex < indices.Length; primitiveIndex++)
                mesh.SetTriangles(indices[primitiveIndex], primitiveIndex, false);
            if (uvs != null) mesh.uv = uvs;
            var featureUvs = new Vector2[featureIds.Length];
            for (var index = 0; index < featureUvs.Length; index++) featureUvs[index] = new Vector2(featureIds[index], 0f);
            mesh.uv2 = featureUvs;
            mesh.RecalculateBounds();
            return new DecodedTileMesh(mesh, featureIds, BuildTriangleFeatureIds(indices, featureIds), textures);
        }

        private static ushort[] BuildTriangleFeatureIds(int[][] indices, ushort[] featureIds)
        {
            var values = new System.Collections.Generic.List<ushort>();
            foreach (var submesh in indices)
            {
                for (var index = 0; index < submesh.Length; index += 3)
                {
                    var featureId = featureIds[submesh[index]];
                    if (featureIds[submesh[index + 1]] != featureId || featureIds[submesh[index + 2]] != featureId)
                        throw new InvalidDataException("Every triangle must have one tile-local feature ID.");
                    values.Add(featureId);
                }
            }
            return values.ToArray();
        }

        private static Vector2[] ReadVector2Accessor(GlbRoot root, byte[] binary, int accessorIndex, string name)
        {
            var accessor = GetAccessor(root, accessorIndex, name);
            if (accessor.componentType != 5126 || accessor.type != "VEC2") throw new InvalidDataException(name + " must be FLOAT VEC2.");
            var view = GetBufferView(root, accessor.bufferView, name);
            var stride = view.byteStride == 0 ? 8 : view.byteStride;
            if (stride < 8) throw new InvalidDataException(name + " stride is too small.");
            var values = new Vector2[accessor.count];
            for (var i = 0; i < values.Length; i++)
            {
                var entry = GetEntryOffset(view, accessor, binary, stride, i, 8, name);
                values[i] = new Vector2(ReadSingle(binary, entry), ReadSingle(binary, entry + 4));
            }
            return values;
        }

        private static Texture2D[] ReadPrimitiveTextures(GlbRoot root, byte[] binary, GlbPrimitive[] primitives)
        {
            var values = new Texture2D[primitives.Length];
            try
            {
                for (var index = 0; index < primitives.Length; index++)
                {
                    var material = GetMaterial(root, primitives[index].material);
                    var textureIndex = material.pbrMetallicRoughness?.baseColorTexture?.index ?? -1;
                    if (root.textures == null || textureIndex < 0 || textureIndex >= root.textures.Length) throw new InvalidDataException("Terrain primitive base color texture is missing.");
                    var imageIndex = root.textures[textureIndex]?.source ?? -1;
                    if (root.images == null || imageIndex < 0 || imageIndex >= root.images.Length) throw new InvalidDataException("Terrain texture image is missing.");
                    values[index] = DecodeImage(root, binary, root.images[imageIndex]);
                }
                return values;
            }
            catch
            {
                foreach (var texture in values) if (texture != null) UnityEngine.Object.Destroy(texture);
                throw;
            }
        }

        private static GlbMaterial GetMaterial(GlbRoot root, int index)
        {
            if (root.materials == null || index < 0 || index >= root.materials.Length || root.materials[index] == null)
                throw new InvalidDataException("Terrain primitive material is missing.");
            return root.materials[index];
        }

        private static Texture2D DecodeImage(GlbRoot root, byte[] binary, GlbImage image)
        {
            if (image == null || !string.IsNullOrEmpty(image.uri) || (image.mimeType != "image/png" && image.mimeType != "image/jpeg"))
                throw new InvalidDataException("Terrain images must be embedded PNG or JPEG data.");
            var view = GetBufferView(root, image.bufferView, "terrain image");
            if (view.byteStride != 0 || view.byteLength <= 0 || view.byteLength > MaxImageBytes || view.byteOffset > binary.Length - view.byteLength)
                throw new InvalidDataException("Terrain image bufferView is invalid.");
            var bytes = new byte[view.byteLength];
            Buffer.BlockCopy(binary, view.byteOffset, bytes, 0, bytes.Length);
            ValidateImageHeader(bytes, image.mimeType);
            var texture = new Texture2D(2, 2, TextureFormat.RGBA32, true, false) { name = "CityModel Terrain Texture" };
            if (!ImageConversion.LoadImage(texture, bytes, true) || texture.width > MaxImageDimension || texture.height > MaxImageDimension)
            {
                UnityEngine.Object.Destroy(texture);
                throw new InvalidDataException("Terrain image cannot be decoded within dimension limits.");
            }
            texture.wrapMode = TextureWrapMode.Clamp;
            texture.filterMode = FilterMode.Bilinear;
            return texture;
        }

        private static void ValidateImageHeader(byte[] bytes, string mimeType)
        {
            if (mimeType == "image/png")
            {
                if (bytes.Length < 24 || bytes[0] != 137 || bytes[1] != 80 || bytes[2] != 78 || bytes[3] != 71 || bytes[4] != 13 || bytes[5] != 10 || bytes[6] != 26 || bytes[7] != 10)
                    throw new InvalidDataException("Terrain image MIME type does not match PNG data.");
                var width = ReadBigEndianUInt32(bytes, 16); var height = ReadBigEndianUInt32(bytes, 20);
                if (width == 0 || height == 0 || width > MaxImageDimension || height > MaxImageDimension) throw new InvalidDataException("Terrain PNG dimensions exceed limits.");
                return;
            }
            if (bytes.Length < 4 || bytes[0] != 0xff || bytes[1] != 0xd8) throw new InvalidDataException("Terrain image MIME type does not match JPEG data.");
            var offset = 2;
            while (offset + 9 < bytes.Length)
            {
                if (bytes[offset++] != 0xff) continue;
                while (offset < bytes.Length && bytes[offset] == 0xff) offset++;
                if (offset >= bytes.Length) break;
                var marker = bytes[offset++];
                if (marker == 0xd9 || marker == 0xda) break;
                if (offset + 2 > bytes.Length) break;
                var length = (bytes[offset] << 8) | bytes[offset + 1];
                if (length < 2 || offset + length > bytes.Length) break;
                if ((marker >= 0xc0 && marker <= 0xc3) || (marker >= 0xc5 && marker <= 0xc7) || (marker >= 0xc9 && marker <= 0xcb) || (marker >= 0xcd && marker <= 0xcf))
                {
                    var height = (bytes[offset + 3] << 8) | bytes[offset + 4]; var width = (bytes[offset + 5] << 8) | bytes[offset + 6];
                    if (width == 0 || height == 0 || width > MaxImageDimension || height > MaxImageDimension) throw new InvalidDataException("Terrain JPEG dimensions exceed limits.");
                    return;
                }
                offset += length;
            }
            throw new InvalidDataException("Terrain JPEG dimensions are invalid.");
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

        private static ushort[] ReadFeatureIds(GlbRoot root, byte[] binary, int accessorIndex, int vertexCount)
        {
            var values = new ushort[vertexCount];
            if (accessorIndex < 0)
            {
                for (var index = 0; index < values.Length; index++) values[index] = ushort.MaxValue;
                return values;
            }
            var accessor = GetAccessor(root, accessorIndex, "_FEATURE_ID_0");
            if (accessor.componentType != 5123 || accessor.type != "SCALAR" || accessor.count != vertexCount)
                throw new InvalidDataException("_FEATURE_ID_0 must be UNSIGNED_SHORT with one value per vertex.");
            var view = GetBufferView(root, accessor.bufferView, "_FEATURE_ID_0");
            var stride = view.byteStride == 0 ? 2 : view.byteStride;
            if (stride < 2) throw new InvalidDataException("_FEATURE_ID_0 stride is too small.");
            for (var i = 0; i < accessor.count; i++)
            {
                var entry = GetEntryOffset(view, accessor, binary, stride, i, 2, "_FEATURE_ID_0");
                values[i] = (ushort)(binary[entry] | binary[entry + 1] << 8);
            }
            return values;
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
        private static uint ReadBigEndianUInt32(byte[] bytes, int offset)
        {
            if (offset < 0 || offset > bytes.Length - 4) throw new InvalidDataException("Image data is truncated.");
            return (uint)(bytes[offset] << 24 | bytes[offset + 1] << 16 | bytes[offset + 2] << 8 | bytes[offset + 3]);
        }
        private static float ReadSingle(byte[] bytes, int offset)
        {
            if (offset < 0 || offset > bytes.Length - 4) throw new InvalidDataException("GLB data is truncated.");
            return BitConverter.ToSingle(bytes, offset);
        }
    }
}
