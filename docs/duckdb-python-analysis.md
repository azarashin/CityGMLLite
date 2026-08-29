# Python と DuckDB で変換済みデータを分析する

この資料は、Unity で利用する SQLite 成果物を Python の DuckDB から分析するための独立したメモです。converter や Unity プロジェクトに DuckDB を組み込む必要はありません。

## 前提

変換後の出力ディレクトリにある `citymodel.sqlite` を使用します。SQLite は分析中に変更せず、読み取り専用で接続してください。

必要なパッケージをインストールします。

```powershell
python -m pip install duckdb pandas
```

## SQLite を読み取り専用で ATTACH する

```python
from pathlib import Path
import duckdb

sqlite_path = Path(r"output/11100-saitama-2025/citymodel.sqlite").resolve()

con = duckdb.connect()
con.execute("INSTALL sqlite")  # 初回のみ。DuckDB の拡張リポジトリへ接続します
con.execute("LOAD sqlite")
con.execute(
    "ATTACH ? AS citymodel (TYPE sqlite, READ_ONLY)",
    [str(sqlite_path)],
)

tables = con.sql("SHOW ALL TABLES FROM citymodel").fetchall()
print(tables)
```

`INSTALL sqlite` はユーザー環境ごとに初回一度だけ実行します。既にインストール済みなら省略できます。

## 集計例

地物種別ごとの件数を集計します。共通スキーマ（`features`）を含む成果物では次のように実行できます。

```python
print(con.sql("""
    SELECT feature_type, COUNT(*) AS feature_count
    FROM citymodel.features
    GROUP BY feature_type
    ORDER BY feature_type
""").fetchdf())
```

建物のタイル別件数と三角形数を確認する例です。

```python
print(con.sql("""
    SELECT b.tile_id,
           COUNT(*) AS building_count,
           MAX(t.triangle_count) AS triangle_count
    FROM citymodel.buildings AS b
    JOIN citymodel.tiles AS t ON t.tile_id = b.tile_id
    GROUP BY b.tile_id
    ORDER BY triangle_count DESC
""").fetchdf())
```

属性値を集計する例です。

```python
print(con.sql("""
    SELECT attribute_key, value_text, COUNT(*) AS count
    FROM citymodel.building_attributes
    GROUP BY attribute_key, value_text
    ORDER BY count DESC
    LIMIT 50
""").fetchdf())
```

`fetchdf()` はクエリ結果を pandas の `DataFrame` として取得します。大きな結果を扱う場合は、`LIMIT`、条件、集計をSQL側で適用してから取得してください。

## 分析用 DuckDB ファイルへ保存する

繰り返し分析する表だけを別の DuckDB ファイルへコピーできます。元のSQLiteは変更されません。

```python
analysis = duckdb.connect("analysis.duckdb")
analysis.execute("INSTALL sqlite")
analysis.execute("LOAD sqlite")
analysis.execute(
    "ATTACH ? AS citymodel (TYPE sqlite, READ_ONLY)",
    [str(sqlite_path)],
)
analysis.execute("""
    CREATE OR REPLACE TABLE feature_attributes AS
    SELECT * FROM citymodel.feature_attributes
""")
analysis.execute("""
    CREATE OR REPLACE TABLE tile_summary AS
    SELECT tile_id, COUNT(*) AS feature_count
    FROM citymodel.features
    GROUP BY tile_id
""")
analysis.close()
```

## 注意事項

- `INSTALL sqlite` は初回に拡張をダウンロードします。ネットワーク制限のある環境では、事前に拡張を配置するか、ネットワークが利用できる環境でインストールを済ませてください。
- 拡張のインストール後は `LOAD sqlite` だけで利用できます。DuckDB本体と拡張のバージョンは揃えてください。
- オフライン環境では、未インストールのsqlite拡張を新たに取得できません。CIや配布環境では、使用するDuckDBバージョンと拡張の準備方法を固定してください。
- SQLiteを `READ_ONLY` で開いても、別プロセスが書き込み中だとロックや未確定状態の影響を受けます。converter完了後、ファイルが確定してから分析してください。
- 大容量の結果を `fetchdf()` で一括取得するとPythonのメモリを消費します。SQL集計、列の限定、分割取得を優先してください。
- UnityランタイムでDuckDBを直接使うことは想定していません。Unityの属性参照は引き続きSQLiteを使用し、DuckDBはPythonによるオフライン分析に限定します。
