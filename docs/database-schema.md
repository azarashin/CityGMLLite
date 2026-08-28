# SQLiteデータベース定義

## 位置づけ

本書は、`contracts/sql/001_initial.sql` に定義された出力SQLiteデータベースを閲覧するためのER図です。SQLマイグレーションを正本とし、本書は設計意図と参照方法を説明します。

## ER図

```mermaid
erDiagram
    dataset_metadata ||--o{ source_files : contains
    dataset_metadata ||--o{ tiles : contains
    source_files ||--o{ conversion_issues : reports
    tiles ||--o{ buildings : contains
    source_files ||--o{ buildings : describes
    buildings ||--o{ building_parts : has
    buildings ||--o{ building_attributes : has
    tiles ||--o{ tile_features : indexes
    buildings ||--o{ tile_features : identified_by
    building_parts ||--o{ tile_features : optionally_identified_by
    buildings ||--o{ conversion_issues : concerns

    dataset_metadata { TEXT dataset_id PK TEXT generation_id UK }
    source_files { INTEGER source_file_id PK TEXT dataset_id FK TEXT relative_path }
    tiles { TEXT tile_id PK TEXT dataset_id FK TEXT glb_relative_path INTEGER building_count }
    buildings { TEXT building_id PK TEXT canonical_building_id UK TEXT tile_id FK INTEGER local_feature_id }
    building_parts { TEXT building_part_id PK TEXT building_id FK }
    building_attributes { INTEGER id PK TEXT building_id FK TEXT attribute_path }
    tile_features { TEXT tile_id PK INTEGER local_feature_id PK TEXT building_id FK }
    conversion_issues { INTEGER id PK INTEGER source_file_id FK TEXT building_id FK TEXT severity }
```

`schema_migrations` はマイグレーション履歴を保持する管理テーブルで、他テーブルからの外部キー参照はありません。

## 現在の対象範囲

現在のスキーマとconverterは建物（`Building` / `BuildingPart`）を対象とします。地形、道路、水部などの非建物地物を格納するテーブルや共通地物IDはまだありません。

将来は共通の `features`（地物種別、恒久ID、タイル内feature IDなど）と `feature_attributes`（名前空間、属性パス、型、値）を導入し、建物固有情報は `buildings` に残す方針です。変更は新しいマイグレーションで行います。

## 関連資料

- [日本語データ辞書](data-dictionary.md)
- [出力データ形式](data-format.md)
- [正本SQL](../contracts/sql/001_initial.sql)
