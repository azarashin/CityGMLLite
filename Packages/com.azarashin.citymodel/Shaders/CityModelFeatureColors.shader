Shader "CityModel/Feature Colors"
{
    Properties
    {
        _CityModelDefaultColor("Default color", Color) = (0.56, 0.59, 0.64, 1)
        [HideInInspector] _CityModelFeatureColorCount("Feature color count", Int) = 0
    }
    SubShader
    {
        Tags { "RenderType" = "Opaque" "Queue" = "Geometry" }
        Pass
        {
            // The Quick Start project uses the Built-in Render Pipeline.  SRPDefaultUnlit
            // is not selected there, so use the Built-in forward pass explicitly.
            Tags { "LightMode" = "ForwardBase" }
            Cull Back
            ZWrite On
            ZTest LEqual
            Blend One Zero
            HLSLPROGRAM
            #pragma target 4.5
            #pragma vertex Vert
            #pragma fragment Frag
            #pragma multi_compile_fwdbase
            #include "UnityCG.cginc"
            #include "Lighting.cginc"
            #include "AutoLight.cginc"

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
                float3 worldNormal : TEXCOORD1;
                SHADOW_COORDS(2)
            };

            Varyings Vert(Attributes input)
            {
                Varyings output;
                output.positionCS = UnityObjectToClipPos(input.positionOS);
                output.featureId = (uint)round(input.featureId.x);
                output.worldNormal = UnityObjectToWorldNormal(input.normalOS);
                TRANSFER_SHADOW(output)
                return output;
            }

            float4 Frag(Varyings input) : SV_Target
            {
                float4 color = _CityModelDefaultColor;
                if (_CityModelFeatureColorCount > 0 && input.featureId < (uint)_CityModelFeatureColorCount)
                {
                    color = _CityModelFeatureColors[input.featureId];
                }

                float3 worldNormal = normalize(input.worldNormal);
                float3 ambient = ShadeSH9(float4(worldNormal, 1.0));
                float lambert = saturate(dot(worldNormal, _WorldSpaceLightPos0.xyz));
                float shadowAttenuation = SHADOW_ATTENUATION(input);
                color.rgb *= ambient + (_LightColor0.rgb * (lambert * shadowAttenuation));

                // Keep the feature layer opaque even if a missing binding or source
                // attribute supplied an unexpected alpha channel.
                color.a = 1.0;
                return color;
            }
            ENDHLSL
        }
    }
}
