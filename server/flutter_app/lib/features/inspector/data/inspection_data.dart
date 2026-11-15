import 'dart:typed_data';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

class InspectionData {
  const InspectionData({
    required this.image,
    required this.elements,
    this.labelsOnImage = false,
    this.elementsError,
    this.display,
  });

  final Uint8List image;
  final bool labelsOnImage;
  final List<AdminElement> elements;
  final String? elementsError;
  final DesktopDisplay? display;
}
