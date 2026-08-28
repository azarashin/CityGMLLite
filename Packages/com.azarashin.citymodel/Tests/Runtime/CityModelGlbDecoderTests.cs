using System;
using System.IO;
using System.Text;
using CityModel.Loading;
using NUnit.Framework;
using UnityEngine;

namespace CityModel.Tests
{
    public sealed class CityModelGlbDecoderTests
    {
        [Test]
        public void DecodeWithFeatureIds_LoadsEmbeddedTerrainTextureAndUvs()
        {
            var decoded = CityModelGlbDecoder.DecodeWithFeatureIds(BuildTexturedTerrainGlb(false), "terrain");
            try
            {
                Assert.That(decoded.Mesh.subMeshCount, Is.EqualTo(1));
                Assert.That(decoded.Mesh.uv, Has.Length.EqualTo(3));
                Assert.That(decoded.FeatureIds, Is.EqualTo(new ushort[] { 0, 0, 0 }));
                Assert.That(decoded.TriangleFeatureIds, Is.EqualTo(new ushort[] { 0 }));
                Assert.That(decoded.Textures, Has.Length.EqualTo(1));
                Assert.That(decoded.Textures[0].width, Is.EqualTo(1));
                Assert.That(decoded.Textures[0].height, Is.EqualTo(1));
            }
            finally
            {
                foreach (var texture in decoded.Textures) UnityEngine.Object.DestroyImmediate(texture);
                UnityEngine.Object.DestroyImmediate(decoded.Mesh);
            }
        }

        [Test]
        public void DecodeWithFeatureIds_RejectsExternalTerrainImageUri()
        {
            Assert.Throws<InvalidDataException>(() => CityModelGlbDecoder.DecodeWithFeatureIds(BuildTexturedTerrainGlb(true), "terrain"));
        }

        private static byte[] BuildTexturedTerrainGlb(bool externalImage)
        {
            var binary = new MemoryStream();
            WriteFloat(binary, 0); WriteFloat(binary, 0); WriteFloat(binary, 0);
            WriteFloat(binary, 1); WriteFloat(binary, 0); WriteFloat(binary, 0);
            WriteFloat(binary, 0); WriteFloat(binary, 1); WriteFloat(binary, 0);
            for (var index = 0; index < 3; index++) { WriteFloat(binary, 0); WriteFloat(binary, 0); WriteFloat(binary, 1); }
            WriteFloat(binary, 0); WriteFloat(binary, 0); WriteFloat(binary, 1); WriteFloat(binary, 0); WriteFloat(binary, 0); WriteFloat(binary, 1);
            binary.WriteByte(0); binary.WriteByte(0); binary.WriteByte(0); binary.WriteByte(0); binary.WriteByte(0); binary.WriteByte(0);
            WriteUInt32(binary, 0); WriteUInt32(binary, 1); WriteUInt32(binary, 2);
            while (binary.Length % 4 != 0) binary.WriteByte(0);
            var imageOffset = binary.Length;
            var png = Convert.FromBase64String("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScL7XQAAAABJRU5ErkJggg==");
            binary.Write(png, 0, png.Length);
            while (binary.Length % 4 != 0) binary.WriteByte(0);
            var image = externalImage
                ? "{\\\"uri\\\":\\\"https://example.test/terrain.png\\\",\\\"mimeType\\\":\\\"image/png\\\"}"
                : "{\\\"bufferView\\\":5,\\\"mimeType\\\":\\\"image/png\\\"}";
            var json = "{\\\"asset\\\":{\\\"version\\\":\\\"2.0\\\"},\\\"buffers\\\":[{\\\"byteLength\\\":" + binary.Length + "}],\\\"bufferViews\\\":[{\\\"buffer\\\":0,\\\"byteOffset\\\":0,\\\"byteLength\\\":36},{\\\"buffer\\\":0,\\\"byteOffset\\\":36,\\\"byteLength\\\":36},{\\\"buffer\\\":0,\\\"byteOffset\\\":72,\\\"byteLength\\\":24},{\\\"buffer\\\":0,\\\"byteOffset\\\":96,\\\"byteLength\\\":6},{\\\"buffer\\\":0,\\\"byteOffset\\\":102,\\\"byteLength\\\":12},{\\\"buffer\\\":0,\\\"byteOffset\\\":" + imageOffset + ",\\\"byteLength\\\":" + png.Length + "}],\\\"accessors\\\":[{\\\"bufferView\\\":0,\\\"componentType\\\":5126,\\\"count\\\":3,\\\"type\\\":\\\"VEC3\\\"},{\\\"bufferView\\\":1,\\\"componentType\\\":5126,\\\"count\\\":3,\\\"type\\\":\\\"VEC3\\\"},{\\\"bufferView\\\":2,\\\"componentType\\\":5126,\\\"count\\\":3,\\\"type\\\":\\\"VEC2\\\"},{\\\"bufferView\\\":3,\\\"componentType\\\":5123,\\\"count\\\":3,\\\"type\\\":\\\"SCALAR\\\"}],\\\"images\\\":[" + image + "],\\\"textures\\\":[{\\\"source\\\":0}],\\\"materials\\\":[{\\\"pbrMetallicRoughness\\\":{\\\"baseColorTexture\\\":{\\\"index\\\":0}}}],\\\"meshes\\\":[{\\\"primitives\\\":[{\\\"attributes\\\":{\\\"POSITION\\\":0,\\\"NORMAL\\\":1,\\\"TEXCOORD_0\\\":2,\\\"_FEATURE_ID_0\\\":3},\\\"indices\\\":4,\\\"material\\\":0,\\\"mode\\\":4}]}]}";
            // The compact fixture uses escaped quotes to keep this test readable;
            // the GLB JSON itself must contain ordinary JSON quotes.
            return BuildGlb(Encoding.UTF8.GetBytes(json.Replace("\\\"", "\"")), binary.ToArray());
        }

        private static byte[] BuildGlb(byte[] json, byte[] binary)
        {
            using (var stream = new MemoryStream())
            {
                while (json.Length % 4 != 0) Array.Resize(ref json, json.Length + 1);
                while (binary.Length % 4 != 0) Array.Resize(ref binary, binary.Length + 1);
                WriteUInt32(stream, 0x46546c67); WriteUInt32(stream, 2); WriteUInt32(stream, (uint)(12 + 8 + json.Length + 8 + binary.Length));
                WriteUInt32(stream, (uint)json.Length); WriteUInt32(stream, 0x4e4f534a); stream.Write(json, 0, json.Length);
                WriteUInt32(stream, (uint)binary.Length); WriteUInt32(stream, 0x004e4942); stream.Write(binary, 0, binary.Length);
                return stream.ToArray();
            }
        }

        private static void WriteFloat(Stream stream, float value) { var bytes = BitConverter.GetBytes(value); stream.Write(bytes, 0, bytes.Length); }
        private static void WriteUInt32(Stream stream, uint value) { var bytes = BitConverter.GetBytes(value); stream.Write(bytes, 0, bytes.Length); }
    }
}
