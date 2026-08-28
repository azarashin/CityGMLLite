Shader "CityModel/Feature Colors"
{
    Properties
    {
        _CityModelDefaultColor("Default color", Color) = (0.56, 0.59, 0.64, 1)
    }
    SubShader
    {
        Tags { "RenderType" = "Opaque" "Queue" = "Geometry" }
        Pass
        {
            Tags { "LightMode" = "SRPDefaultUnlit" }
            HLSLPROGRAM
            #pragma target 4.5
            #pragma vertex Vert
            #pragma fragment Frag
            #include "UnityCG.cginc"

            StructuredBuffer<float4> _CityModelFeatureColors;
            int _CityModelFeatureColorCount;
            float4 _CityModelDefaultColor;

            struct Attributes
            {
                float4 positionOS : POSITION;
                float2 featureId : TEXCOORD1;
            };

            struct Varyings
            {
                float4 positionCS : SV_POSITION;
                nointerpolation uint featureId : TEXCOORD0;
            };

            Varyings Vert(Attributes input)
            {
                Varyings output;
                output.positionCS = UnityObjectToClipPos(input.positionOS);
                output.featureId = (uint)round(input.featureId.x);
                return output;
            }

            float4 Frag(Varyings input) : SV_Target
            {
                return input.featureId < (uint)_CityModelFeatureColorCount
                    ? _CityModelFeatureColors[input.featureId]
                    : _CityModelDefaultColor;
            }
            ENDHLSL
        }
    }
}
