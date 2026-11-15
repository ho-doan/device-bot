// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'web_context.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

WebFrameSelector _$WebFrameSelectorFromJson(Map<String, dynamic> json) =>
    WebFrameSelector(
      css: json['css'] as String?,
      name: json['name'] as String?,
      url: json['url'] as String?,
      position: (json['position'] as num?)?.toInt(),
    );

Map<String, dynamic> _$WebFrameSelectorToJson(WebFrameSelector instance) =>
    <String, dynamic>{
      'css': ?instance.css,
      'name': ?instance.name,
      'url': ?instance.url,
      'position': ?instance.position,
    };

WebContextPayload _$WebContextPayloadFromJson(Map<String, dynamic> json) =>
    WebContextPayload(
      elementRef: json['elementRef'] as String?,
      appId: json['appId'] as String?,
      page: _pageOrNull(json['page']),
      framePath: json['framePath'] == null
          ? const []
          : _framesFromJson(json['framePath']),
    );

Map<String, dynamic> _$WebContextPayloadToJson(WebContextPayload instance) =>
    <String, dynamic>{
      'elementRef': ?instance.elementRef,
      'appId': ?instance.appId,
      'page': ?instance.page?.toJson(),
      'framePath': instance.framePath.map((e) => e.toJson()).toList(),
    };

WebPage _$WebPageFromJson(Map<String, dynamic> json) =>
    WebPage(url: json['url'] as String?, title: json['title'] as String?);

Map<String, dynamic> _$WebPageToJson(WebPage instance) => <String, dynamic>{
  'url': ?instance.url,
  'title': ?instance.title,
};
