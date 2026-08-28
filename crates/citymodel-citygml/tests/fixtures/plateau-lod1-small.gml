<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:b="http://www.opengis.net/citygml/building/2.0" xmlns:g="http://www.opengis.net/gml">
  <core:cityObjectMember>
    <b:Building g:id="sample-building-1">
      <b:usage codeSpace="https://example.test/usage">residential</b:usage>
      <b:measuredHeight uom="m">12.5</b:measuredHeight>
      <b:lod1Solid>
        <g:Solid srsName="urn:ogc:def:crs:EPSG::6697" srsDimension="3">
          <g:exterior><g:CompositeSurface><g:surfaceMember><g:Polygon><g:exterior><g:LinearRing>
            <g:posList>35.0 139.0 10.0 35.0 139.1 10.0 35.1 139.1 10.0 35.0 139.0 10.0</g:posList>
          </g:LinearRing></g:exterior></g:Polygon></g:surfaceMember></g:CompositeSurface></g:exterior>
        </g:Solid>
      </b:lod1Solid>
    </b:Building>
  </core:cityObjectMember>
</core:CityModel>
