import 'package:flutter/widgets.dart';

/// Attaches an automation ID to one control's existing accessible node.
///
/// Merging keeps the control's label, actions and state on the identified node.
/// Do not use this around a subtree containing independently actionable controls.
class IdentifiedControl extends StatelessWidget {
  const IdentifiedControl({
    super.key,
    required this.identifier,
    required this.child,
  });

  final String identifier;
  final Widget child;

  @override
  Widget build(BuildContext context) => MergeSemantics(
    child: Semantics(identifier: identifier, child: child),
  );
}
