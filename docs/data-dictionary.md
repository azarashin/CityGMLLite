# SQLiteデータ辞書

データ型と制約の正本は [`contracts/sql/001_initial.sql`](../contracts/sql/001_initial.sql) です。以下では、検索・属性表示・変換結果の監査に必要な項目を説明します。

## 管理・入力情報

| テーブル | カラム | 説明 |
|---|---|---|
| `schema_migrations` | `version`, `applied_at`, `description` | 適用済みマイグレーション番号、日時、説明 |
| `dataset_metadata` | `dataset_id`, `schema_version`, `generation_id` | データセット、スキーマ、生成処理の識別子 |
|  | `generated_at`, `generator_name`, `generator_version` | 生成日時、プログラム名、バージョン |
|  | `source_crs_*`, `working_crs_*`, `vertical_*` | 入力・作業・高さ座標系 |
|  | `axis_order_json`, `dataset_origin_*` | 軸順序とデータセット原点 |
|  | `manifest_sha256`, `database_sha256` | manifest／DBのSHA-256 |
|  | `conversion_config_json`, `license_json` | 変換設定／ライセンスのJSON |
| `source_files` | `source_file_id`, `dataset_id`, `relative_path` | 入力ファイル識別子、所属、相対パス |
|  | `sha256`, `byte_length` | 入力ファイルのハッシュ、バイト数 |

## タイル・建物

| テーブル | カラム | 説明 |
|---|---|---|
| `tiles` | `tile_id`, `dataset_id`, `generation_id` | タイル、データセット、生成処理の識別子 |
|  | `glb_relative_path`, `metadata_relative_path` | GLB／メタデータの相対パス |
|  | `glb_sha256`, `glb_byte_length` | GLBのハッシュ、バイト数 |
|  | `origin_*`, `tile_min_*`, `tile_max_*` | 原点とタイル境界 |
|  | `content_min_*`, `content_max_*` | コンテンツ境界 |
|  | `projected_to_local_matrix_json` | 投影座標からローカル座標への変換行列 |
|  | `building_count`, `vertex_count`, `triangle_count`, `primitive_count` | タイル内の統計値 |
| `buildings` | `building_id`, `canonical_building_id` | 内部ID、正規化ID |
|  | `gml_id`, `id_source`, `id_is_synthetic` | 原始GML ID、採用元、合成IDフラグ |
|  | `source_file_id`, `tile_id`, `local_feature_id` | 入力、所属タイル、GLB内feature ID |
|  | `lod_used`, `lod_generated` | 採用LOD、LOD補間生成フラグ |
|  | `measured_height`, `min_height`, `max_height` | 高さ属性 |
|  | `centroid_x`, `centroid_y`, `centroid_lon`, `centroid_lat` | 重心の作業／地理座標 |
|  | `footprint_quality`, `attributes_json`, `footprint` | 品質、互換用属性JSON、フットプリント |
| `building_parts` | `building_part_id`, `building_id`, `gml_id` | 部品ID、親建物、部品のGML ID |

## 属性・描画対応・変換問題

| テーブル | カラム | 説明 |
|---|---|---|
| `building_attributes` | `id`, `building_id` | 属性行ID、対象建物 |
|  | `namespace_uri`, `attribute_path`, `attribute_key`, `ordinal` | 名前空間、XMLパス、属性名、出現順 |
|  | `value_type`, `value_text`, `value_integer`, `value_real`, `value_boolean`, `value_datetime` | 型と型別の属性値 |
|  | `uom`, `code_space`, `nil_reason` | 単位、コード体系、欠損理由 |
| `tile_features` | `tile_id`, `local_feature_id` | タイルとGLB内feature ID（複合主キー） |
|  | `building_id`, `building_part_id` | 対応する建物／部品 |
| `conversion_issues` | `id`, `source_file_id`, `building_id`, `gml_id` | 問題IDと対象識別子 |
|  | `severity`, `error_code`, `message`, `element_path` | 重要度、コード、内容、XMLパス |
|  | `repaired`, `exclusion_reason`, `occurred_at` | 修復済み、除外理由、発生日時 |

## 将来の拡張

地形（`dem`）、水部（`wtr`）、道路（`tran`）などは、建物専用のテーブルを直接流用せず、共通 `features` と `feature_attributes` を追加します。クリック時は、GLBのタイルローカルfeature IDから共通地物IDを解決し、そのIDで属性を検索する構成を想定します。
