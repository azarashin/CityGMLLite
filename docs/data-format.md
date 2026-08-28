# Output data format

`dataset.manifest.json` is the dataset entry point. Each tile has a neighboring
`.meta.json` file and a GLB. Both JSON documents must validate against the schemas
under `contracts/schemas/`; their `generationId` values must agree with GLB extras.

Feature IDs are tile-local `UNSIGNED_SHORT` values stored as `_FEATURE_ID_0` and map
by index to the metadata `features.buildingIds` array. The SQLite schema and migration
are the authoritative database definition in `contracts/sql/001_initial.sql`.

データベースのER図と日本語データ辞書は、[SQLiteデータベース定義](database-schema.md)および[SQLiteデータ辞書](data-dictionary.md)を参照してください。建物は従来テーブルに保持し、種別共通の参照は `features` / `feature_attributes` / `feature_tile_mappings` を使用します。

## 地形コンテンツ

converter は PLATEAU / CityGML 2.0 の `udx/dem` にある `dem:ReliefFeature` と `dem:TINRelief` を探索します。`app:ParameterizedTexture` が面（または `LinearRing`）の `gml:id` を `target` として参照し、`imageURI` と UV を持つ場合、`terrain/<tileId>.glb` を独立した `featureType: "terrain"` コンテンツとして出力します。

- terrain GLB は `POSITION`、`NORMAL`、`TEXCOORD_0`、`_FEATURE_ID_0` と glTF material を持ちます。
- PNG/JPEG は外部URIとしては出力せず、GLBのbuffer viewへ埋め込みます。
- `imageURI` は入力ルート配下の相対パスだけを許可します。絶対パス、`..`、URLは拒否します。PNG/JPEGヘッダと最大64 MiB・16,384pxの上限を検証します。
- texture target がない地形面は出力しません。対応付いた面でUV数が頂点数と一致しない場合は変換を失敗させます。
