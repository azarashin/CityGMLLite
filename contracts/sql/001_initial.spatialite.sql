-- CityGMLLite SpatiaLite extension for 001_initial.sql.
--
-- The converter must execute this after loading its pinned SpatiaLite bridge and
-- substitute @working_srid@ with the selected integer Working CRS EPSG code.
-- Do not expose either statement through the Unity public API.

SELECT InitSpatialMetaData(1);
SELECT RecoverGeometryColumn('buildings', 'footprint', @working_srid@, 'MULTIPOLYGON', 'XY');
SELECT CreateSpatialIndex('buildings', 'footprint');
