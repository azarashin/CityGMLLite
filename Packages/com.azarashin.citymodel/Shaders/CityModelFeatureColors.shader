Shader "CityModel/Feature Colors"
{
    Properties
    {
        _CityModelDefaultColor("Default color", Color) = (0.56, 0.59, 0.64, 1)
        [HideInInspector] _CityModelFeatureColorCount("Feature color count", Int) = 0
    }
    SubShader
    {
        Tags { "RenderType" = "Opaque" "Queue" = "Geometry" "RenderPipeline" = "UniversalPipeline" }
        Pass
        {
            Tags { "LightMode" = "UniversalForward" }
            Cull Back
            ZWrite On
            ZTest LEqual
            Blend One Zero
            HLSLPROGRAM
            #pragma target 4.5
            #pragma vertex Vert
            #pragma fragment Frag
            #pragma multi_compile _ _MAIN_LIGHT_SHADOWS _MAIN_LIGHT_SHADOWS_CASCADE _MAIN_LIGHT_SHADOWS_SCREEN
            #pragma multi_compile _ _SHADOWS_SOFT
            #include "Packages/com.unity.render-pipelines.universal/ShaderLibrary/Core.hlsl"
            #include "Packages/com.unity.render-pipelines.universal/ShaderLibrary/Lighting.hlsl"

            StructuredBuffer<float4> _CityModelFeatureColors;
            int _CityModelFeatureColorCount;
            float4 _CityModelDefaultColor;

            struct Attributes
            {
                float4 positionOS : POSITION;
                float3 normalOS : NORMAL;
                float2 featureId : TEXCOORD1;
            };

            struct Varyings
            {
                float4 positionCS : SV_POSITION;
                nointerpolation uint featureId : TEXCOORD0;
                float3 positionWS : TEXCOORD1;
                float3 normalWS : TEXCOORD2;
            };

            Varyings Vert(Attributes input)
            {
                Varyings output;
                VertexPositionInputs positionInputs = GetVertexPositionInputs(input.positionOS.xyz);
                VertexNormalInputs normalInputs = GetVertexNormalInputs(input.normalOS);
                output.positionCS = positionInputs.positionCS;
                output.featureId = (uint)round(input.featureId.x);
                output.positionWS = positionInputs.positionWS;
                output.normalWS = normalInputs.normalWS;
                return output;
            }

            float4 Frag(Varyings input) : SV_Target
            {
                float4 color = _CityModelDefaultColor;
                if (_CityModelFeatureColorCount > 0 && input.featureId < (uint)_CityModelFeatureColorCount)
                {
                    color = _CityModelFeatureColors[input.featureId];
                }

                float3 worldNormal = normalize(input.normalWS);
                Light mainLight = GetMainLight(TransformWorldToShadowCoord(input.positionWS));
                float3 ambient = SampleSH(worldNormal);
                float lambert = saturate(dot(worldNormal, mainLight.direction));
                color.rgb *= ambient + (mainLight.color * (lambert * mainLight.shadowAttenuation));

                // Keep the feature layer opaque even if a missing binding or source
                // attribute supplied an unexpected alpha channel.
                color.a = 1.0;
                return color;
            }
            ENDHLSL
        }
    }
}
