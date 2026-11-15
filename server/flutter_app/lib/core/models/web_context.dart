import 'package:json_annotation/json_annotation.dart';

part 'web_context.g.dart';

@JsonSerializable(includeIfNull: false)
class WebFrameSelector {
  const WebFrameSelector({this.css, this.name, this.url, this.position});

  factory WebFrameSelector.fromJson(Map<String, dynamic> json) =>
      _$WebFrameSelectorFromJson(json);

  final String? css;
  final String? name;
  final String? url;
  final int? position;

  Map<String, dynamic> toJson() => _$WebFrameSelectorToJson(this);
}

class WebContext {
  const WebContext({
    this.elementRef,
    this.appId,
    this.pageUrl,
    this.pageTitle,
    this.framePath = const [],
  });

  factory WebContext.fromJson(Map<String, dynamic> json) {
    final value = WebContextPayload.fromJson(json);
    return WebContext(
      elementRef: value.elementRef,
      appId: value.appId,
      pageUrl: value.page?.url,
      pageTitle: value.page?.title,
      framePath: value.framePath,
    );
  }

  final String? elementRef;
  final String? appId;
  final String? pageUrl;
  final String? pageTitle;
  final List<WebFrameSelector> framePath;

  @JsonKey(includeFromJson: false, includeToJson: false)
  bool get isEmpty =>
      (appId == null || appId!.isEmpty) &&
      (pageUrl == null || pageUrl!.isEmpty) &&
      (pageTitle == null || pageTitle!.isEmpty) &&
      framePath.isEmpty;

  @JsonKey(includeFromJson: false, includeToJson: false)
  String get identity => [
    appId ?? '',
    pageUrl ?? '',
    pageTitle ?? '',
    for (final frame in framePath)
      '${frame.css}|${frame.name}|${frame.url}|${frame.position}',
  ].join('\n');

  Map<String, dynamic> toJson() => WebContextPayload(
    elementRef: elementRef,
    appId: appId,
    page: pageUrl == null && pageTitle == null
        ? null
        : WebPage(url: pageUrl, title: pageTitle),
    framePath: framePath,
  ).toJson();

  String toYaml(String indent) {
    final lines = <String>['${indent}web_context:'];
    if (appId != null && appId!.isNotEmpty) {
      lines.add('$indent  app_id: "${_yaml(appId!)}"');
    }
    if ((pageUrl != null && pageUrl!.isNotEmpty) ||
        (pageTitle != null && pageTitle!.isNotEmpty)) {
      lines.add('$indent  page:');
      if (pageUrl != null && pageUrl!.isNotEmpty) {
        lines.add('$indent    url: "${_yaml(pageUrl!)}"');
      }
      if (pageTitle != null && pageTitle!.isNotEmpty) {
        lines.add('$indent    title: "${_yaml(pageTitle!)}"');
      }
    }
    if (framePath.isNotEmpty) {
      lines.add('$indent  frame_path:');
      for (final frame in framePath) {
        final fields = <String>[
          if (frame.css != null && frame.css!.isNotEmpty)
            'css: "${_yaml(frame.css!)}"',
          if (frame.name != null && frame.name!.isNotEmpty)
            'name: "${_yaml(frame.name!)}"',
          if (frame.url != null && frame.url!.isNotEmpty)
            'url: "${_yaml(frame.url!)}"',
          if (frame.position != null) 'position: ${frame.position}',
        ];
        lines.add('$indent    - ${fields.join('\n$indent      ')}');
      }
    }
    return lines.join('\n');
  }
}

String _yaml(String value) =>
    value.replaceAll('\\', '\\\\').replaceAll('"', '\\"').replaceAll('\n', ' ');

@JsonSerializable(explicitToJson: true, includeIfNull: false)
class WebContextPayload {
  const WebContextPayload({
    this.elementRef,
    this.appId,
    this.page,
    this.framePath = const [],
  });
  factory WebContextPayload.fromJson(Map<String, dynamic> json) =>
      _$WebContextPayloadFromJson(json);
  Map<String, dynamic> toJson() => _$WebContextPayloadToJson(this);
  final String? elementRef, appId;
  @JsonKey(fromJson: _pageOrNull)
  final WebPage? page;
  @JsonKey(fromJson: _framesFromJson)
  final List<WebFrameSelector> framePath;
}

@JsonSerializable(includeIfNull: false)
class WebPage {
  const WebPage({this.url, this.title});
  factory WebPage.fromJson(Map<String, dynamic> json) =>
      _$WebPageFromJson(json);
  Map<String, dynamic> toJson() => _$WebPageToJson(this);
  final String? url, title;
}

List<WebFrameSelector> _framesFromJson(Object? value) => value is List
    ? [
        for (final frame in value)
          if (frame is Map)
            WebFrameSelector.fromJson(Map<String, dynamic>.from(frame)),
      ]
    : const [];

WebPage? _pageOrNull(Object? value) =>
    value is Map ? WebPage.fromJson(Map<String, dynamic>.from(value)) : null;
