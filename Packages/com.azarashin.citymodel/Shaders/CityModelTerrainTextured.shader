Shader "CityModel/Terrain Textured"
{
    Properties { _MainTex("Terrain Texture", 2D) = "white" {} }
    SubShader
    {
        Tags { "RenderType" = "Opaque" "Queue" = "Geometry" }
        Pass
        {
            Tags { "LightMode" = "ForwardBase" }
            Cull Back
            ZWrite On
            HLSLPROGRAM
            #pragma target 3.0
            #pragma vertex Vert
            #pragma fragment Frag
            #pragma multi_compile_fwdbase
            #include "UnityCG.cginc"
            #include "Lighting.cginc"
            #include "AutoLight.cginc"

            sampler2D _MainTex;
            struct Attributes { float4 positionOS : POSITION; float3 normalOS : NORMAL; float2 uv : TEXCOORD0; };
            struct Varyings { float4 positionCS : SV_POSITION; float2 uv : TEXCOORD0; float3 worldNormal : TEXCOORD1; SHADOW_COORDS(2) };
            Varyings Vert(Attributes input)
            {
                Varyings output;
                output.positionCS = UnityObjectToClipPos(input.positionOS);
                output.uv = input.uv;
                output.worldNormal = UnityObjectToWorldNormal(input.normalOS);
                TRANSFER_SHADOW(output)
                return output;
            }
            float4 Frag(Varyings input) : SV_Target
            {
                float4 color = tex2D(_MainTex, input.uv);
                float3 worldNormal = normalize(input.worldNormal);
                float3 ambient = ShadeSH9(float4(worldNormal, 1.0));
                float lambert = saturate(dot(worldNormal, _WorldSpaceLightPos0.xyz));
                color.rgb *= ambient + (_LightColor0.rgb * (lambert * SHADOW_ATTENUATION(input)));
                color.a = 1.0;
                return color;
            }
            ENDHLSL
        }
    }
}
