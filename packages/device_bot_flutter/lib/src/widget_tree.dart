import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

/// Captures the live Element tree without depending on debug-only APIs or UIA.
/// Coordinates use the platform driver units: physical pixels on Windows/Android,
/// desktop points on macOS, UIKit points on iOS, and CSS pixels on web.
class WidgetTreeSnapshot {
  static List<Map<String, Object?>> capture({
    required Offset screenOrigin,
    Element? root,
    double? pixelRatio,
  }) {
    final nodes = <Map<String, Object?>>[];
    var visited = 0;

    void visit(Element element, bool insideControl, bool insideTextField) {
      if (!element.mounted || ++visited > 50000 || nodes.length >= 5000) return;
      final widget = element.widget;
      if (widget is Offstage && widget.offstage ||
          widget is Opacity && widget.opacity == 0) {
        return;
      }

      // Decoration actions have their own control scope. Export the first
      // action, then suppress its nested gesture/semantic implementation nodes.
      if (insideTextField && _isFieldAffix(element)) insideControl = false;
      final node = _describe(element, insideControl, insideTextField);
      if (node != null) {
        // Decorated fields include helper/error text and suffix buttons.
        // Expose the editor's actual hit area so clicks and selection drags do
        // not land on a validation message or the password visibility action.
        final target = widget is TextField
            ? _editableElement(element) ?? element
            : element;
        final bounds = _visibleBounds(target);
        if (bounds != null) {
          final ratio = pixelRatio ?? View.of(element).devicePixelRatio;
          final rect = Rect.fromLTRB(
            screenOrigin.dx + bounds.left * ratio,
            screenOrigin.dy + bounds.top * ratio,
            screenOrigin.dx + bounds.right * ratio,
            screenOrigin.dy + bounds.bottom * ratio,
          );
          nodes.add({
            ...node,
            'widgetType': widget.runtimeType.toString(),
            'resourceId': _identifier(element),
            'left': rect.left.round(),
            'top': rect.top.round(),
            'right': rect.right.round(),
            'bottom': rect.bottom.round(),
          });
        }
      }
      final control =
          insideControl ||
          node?['className'] != null &&
              node!['className'] != 'text' &&
              node['className'] != 'image' &&
              node['className'] != 'scrollView';
      final textField =
          insideTextField || widget is TextField || widget is SelectableText;
      element.visitChildren((child) => visit(child, control, textField));
    }

    final target = root ?? WidgetsBinding.instance.rootElement;
    if (target != null) visit(target, false, false);
    return nodes;
  }

  static Map<String, Object?>? _describe(
    Element element,
    bool insideControl,
    bool insideTextField,
  ) {
    final widget = element.widget;
    // A page-level keyboard dismiss gesture contains independent editors and
    // controls. It must not become one button that suppresses its descendants.
    if (widget is GestureDetector && _editableElement(element) != null) {
      return null;
    }
    String? role;
    String? label;
    String? value;
    bool enabled = true;
    bool clickable = false;
    bool? focused;
    bool? checked;
    bool? selected;
    if (widget is TextField) {
      role = 'textField';
      label =
          widget.decoration?.labelText ??
          _widgetText(widget.decoration?.label) ??
          widget.decoration?.hintText;
      enabled = widget.enabled ?? widget.decoration?.enabled ?? true;
      // EditableText holds the actual controller/focus node, including when
      // TextField created them internally.
      final editor = _editableElement(element)?.widget as EditableText?;
      focused = editor?.focusNode.hasFocus;
      value = editor?.obscureText == false ? editor?.controller.text : null;
      clickable = enabled;
    } else if (widget is EditableText && !insideTextField) {
      role = 'textField';
      focused = widget.focusNode.hasFocus;
      value = widget.obscureText ? null : widget.controller.text;
      clickable = true;
    } else if (widget is ButtonStyleButton &&
        !(insideControl && _hasAncestor<IconButton>(element))) {
      role = 'button';
      label = _widgetText(widget.child);
      enabled = widget.onPressed != null || widget.onLongPress != null;
      clickable = enabled;
      focused = widget.focusNode?.hasFocus;
    } else if (widget is IconButton) {
      role = 'button';
      label = widget.tooltip;
      enabled = widget.onPressed != null;
      clickable = enabled;
      selected = widget.isSelected;
      focused = widget.focusNode?.hasFocus;
    } else if (widget is CheckboxListTile) {
      role = 'checkbox';
      label = _widgetText(widget.title);
      checked = widget.value;
      selected = widget.selected;
      enabled = widget.enabled ?? widget.onChanged != null;
      clickable = enabled;
    } else if (widget is Checkbox && !insideControl) {
      role = 'checkbox';
      checked = widget.value;
      enabled = widget.onChanged != null;
      clickable = enabled;
      focused = widget.focusNode?.hasFocus;
    } else if (widget is SwitchListTile) {
      role = 'switch';
      label = _widgetText(widget.title);
      checked = widget.value;
      selected = widget.selected;
      enabled = widget.onChanged != null;
      clickable = enabled;
    } else if (widget is Switch && !insideControl) {
      role = 'switch';
      checked = widget.value;
      enabled = widget.onChanged != null;
      clickable = enabled;
      focused = widget.focusNode?.hasFocus;
    } else if (widget is Slider) {
      role = 'slider';
      value = widget.value.toString();
      label = widget.label;
      enabled = widget.onChanged != null;
      focused = widget.focusNode?.hasFocus;
    } else if (widget is Tab && _hasAncestor<TabBar>(element)) {
      // TabBar wraps Tab in internal gesture/semantic widgets. Export the
      // public tab itself so its label and hit area survive that control scope.
      role = 'button';
      label = widget.text ?? _widgetText(widget.child);
      clickable = true;
    } else if (widget is ListTile && !insideControl) {
      role = widget.onTap != null ? 'button' : 'listItem';
      label = _widgetText(widget.title);
      enabled = widget.enabled;
      clickable = enabled && widget.onTap != null;
      selected = widget.selected;
    } else if (!insideControl &&
        widget is Semantics &&
        (widget.properties.button == true ||
            widget.properties.checked != null ||
            widget.properties.toggled != null)) {
      final properties = widget.properties;
      role = properties.button == true
          ? 'button'
          : properties.toggled != null
          ? 'switch'
          : properties.inMutuallyExclusiveGroup == true
          ? 'radio'
          : 'checkbox';
      label = properties.label ?? _descendantText(element);
      checked = properties.checked ?? properties.toggled;
      selected = properties.selected;
      focused = properties.focused;
      enabled = properties.enabled ?? true;
      clickable = enabled;
    } else if (widget is Scrollable) {
      role = 'scrollView';
    } else if (!insideControl && widget is SelectableText) {
      role = 'text';
      label = widget.data ?? widget.textSpan?.toPlainText();
    } else if (widget is Text &&
        (!insideControl ||
            insideTextField && _isFieldMessage(element, widget))) {
      role = 'text';
      label = widget.data ?? widget.textSpan?.toPlainText();
    } else if (!insideControl &&
        !insideTextField &&
        widget is RichText &&
        !_hasAncestor<Text>(element)) {
      role = 'text';
      label = widget.text.toPlainText();
    } else if (!insideControl && widget is Image) {
      role = 'image';
      label = widget.semanticLabel;
    } else if (!insideControl &&
        widget is GestureDetector &&
        (widget.onTap != null || widget.onLongPress != null)) {
      role = 'button';
      clickable = true;
      label = _descendantText(element);
    } else if (!insideControl &&
        widget is InkWell &&
        (widget.onTap != null || widget.onLongPress != null)) {
      role = 'button';
      clickable = true;
      label = _widgetText(widget.child);
    }
    if (role == null) return null;
    if (role == 'button' || role == 'listItem') {
      label ??= _descendantText(element);
    }
    label ??= _ancestorLabel(element);
    return {
      'className': role,
      'text': label,
      'contentDescription': label,
      'clickable': clickable,
      'enabled': enabled,
      'focused': focused,
      'checked': checked,
      'selected': selected,
      'value': value,
    };
  }

  static Rect? _visibleBounds(Element element) {
    final render = element.findRenderObject();
    if (render is! RenderBox ||
        !render.attached ||
        !render.hasSize ||
        render.size.isEmpty) {
      return null;
    }
    var rect = MatrixUtils.transformRect(
      render.getTransformTo(null),
      Offset.zero & render.size,
    );
    if (!rect.isFinite || rect.isEmpty) return null;
    RenderObject child = render;
    for (var parent = child.parent; parent != null; parent = child.parent) {
      final clip = parent.describeApproximatePaintClip(child);
      if (clip != null) {
        rect = rect.intersect(
          MatrixUtils.transformRect(parent.getTransformTo(null), clip),
        );
      }
      child = parent;
    }
    final view = View.of(element);
    rect = rect.intersect(
      Offset.zero & (view.physicalSize / view.devicePixelRatio),
    );
    if (rect.isEmpty) return null;
    // Reject widgets covered by another route, dialog, or absorbing overlay.
    final hits = HitTestResult();
    WidgetsBinding.instance.hitTestInView(hits, rect.center, view.viewId);
    final hit = hits.path.any((entry) {
      final target = entry.target;
      if (target is! RenderObject) return false;
      for (
        RenderObject? current = target;
        current != null;
        current = current.parent
      ) {
        if (current == render) return true;
      }
      return false;
    });
    return hit ? rect : null;
  }

  static Element? _editableElement(Element element) {
    Element? result;
    void visit(Element child) {
      if (child.widget is EditableText) {
        result = child;
      } else if (result == null) {
        child.visitChildren(visit);
      }
    }

    element.visitChildren(visit);
    return result;
  }

  static bool _hasAncestor<T extends Widget>(Element element) {
    var found = false;
    element.visitAncestorElements((ancestor) {
      found = ancestor.widget is T;
      return !found;
    });
    return found;
  }

  static bool _isFieldAffix(Element element) {
    var affix = false;
    element.visitAncestorElements((ancestor) {
      final widget = ancestor.widget;
      if (widget is TextField) {
        final decoration = widget.decoration;
        affix =
            decoration != null &&
            [
              decoration.prefixIcon,
              decoration.suffixIcon,
              decoration.prefix,
              decoration.suffix,
            ].any((widget) => identical(widget, element.widget));
        return false;
      }
      return true;
    });
    return affix;
  }

  static bool _isFieldMessage(Element element, Text text) {
    final content = text.data ?? text.textSpan?.toPlainText();
    var message = false;
    element.visitAncestorElements((ancestor) {
      final widget = ancestor.widget;
      if (widget is TextField) {
        message =
            content != null &&
            (content == widget.decoration?.errorText ||
                content == widget.decoration?.helperText);
        return false;
      }
      return true;
    });
    return message;
  }

  static String? _identifier(Element element) {
    String? id(Widget widget) {
      final key = widget.key;
      if (key is ValueKey<String>) return key.value;
      if (widget is Semantics &&
          widget.properties.identifier?.isNotEmpty == true) {
        return widget.properties.identifier;
      }
      return null;
    }

    var result = id(element.widget);
    if (result != null) return result;
    element.visitAncestorElements((ancestor) {
      final widget = ancestor.widget;
      if (widget is Semantics ||
          widget is TextFormField &&
              (element.widget is TextField || element.widget is EditableText)) {
        result = id(widget);
      }
      return result == null && widget is! Form && widget is! Scaffold;
    });
    return result;
  }

  static String? _ancestorLabel(Element element) {
    String? label;
    element.visitAncestorElements((ancestor) {
      final widget = ancestor.widget;
      if (widget is Tooltip) label = widget.message;
      if (widget is Semantics) label ??= widget.properties.label;
      return label == null && widget is! Scaffold;
    });
    return label;
  }

  static String? _widgetText(Widget? widget) {
    if (widget is Text) return widget.data ?? widget.textSpan?.toPlainText();
    if (widget is RichText) return widget.text.toPlainText();
    if (widget is Semantics) {
      return widget.properties.label ?? _widgetText(widget.child);
    }
    if (widget is ProxyWidget) return _widgetText(widget.child);
    if (widget is SingleChildRenderObjectWidget) {
      return _widgetText(widget.child);
    }
    if (widget is MultiChildRenderObjectWidget) {
      final text = widget.children
          .map(_widgetText)
          .whereType<String>()
          .join(' ');
      return text.isEmpty ? null : text;
    }
    return null;
  }

  static String? _descendantText(Element element) {
    final labels = <String>[];
    void visit(Element child) {
      // Icon renders a private font glyph as RichText. It is not a label.
      final widget = child.widget;
      if (widget is Icon) {
        final label = widget.semanticLabel;
        if (label != null && label.isNotEmpty) labels.add(label);
        return;
      }
      final text = _widgetText(widget);
      if (text != null) {
        labels.add(text);
      } else {
        child.visitChildren(visit);
      }
    }

    element.visitChildren(visit);
    return labels.isEmpty ? null : labels.join(' ');
  }
}
