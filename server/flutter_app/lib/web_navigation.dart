import 'package:flutter/material.dart';

/// Opens the URL in the existing case; the dialog never launches another browser.
class WebUrlDialog extends StatefulWidget {
  const WebUrlDialog({
    required this.deviceName,
    required this.onOpen,
    super.key,
  });
  final String deviceName;
  final Future<void> Function(String url) onOpen;
  @override
  State<WebUrlDialog> createState() => _WebUrlDialogState();
}

class _WebUrlDialogState extends State<WebUrlDialog> {
  final _url = TextEditingController();
  bool _opening = false;
  String? _error;
  @override
  void dispose() {
    _url.dispose();
    super.dispose();
  }

  Future<void> _open() async {
    if (_opening) return;
    final value = _url.text.trim();
    final uri = Uri.tryParse(value);
    if (uri == null ||
        !['https', 'http'].contains(uri.scheme) ||
        uri.host.isEmpty ||
        value.contains(RegExp(r'\s'))) {
      setState(
        () => _error =
            'Enter a complete HTTP or HTTPS URL, such as http://localhost:3000.',
      );
      return;
    }
    setState(() {
      _opening = true;
      _error = null;
    });
    try {
      await widget.onOpen(value);
      if (mounted) Navigator.of(context).pop(true);
    } on Object catch (error) {
      if (mounted) setState(() => _error = 'Could not open URL: $error');
    } finally {
      if (mounted) setState(() => _opening = false);
    }
  }

  @override
  Widget build(BuildContext context) => PopScope(
    canPop: !_opening,
    child: AlertDialog(
      title: Text('Open URL on ${widget.deviceName}'),
      content: SizedBox(
        width: 480,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Semantics(
                container: true,
                identifier: 'website-url',
                child: TextField(
                  key: const Key('web-url-input'),
                  controller: _url,
                  autofocus: true,
                  enabled: !_opening,
                  keyboardType: TextInputType.url,
                  decoration: InputDecoration(
                    labelText: 'Website URL',
                    hintText: 'http://localhost:3000',
                    errorText: _error,
                    errorMaxLines: 4,
                  ),
                  onSubmitted: (_) => _open(),
                ),
              ),
              if (_error != null)
                Semantics(
                  liveRegion: true,
                  container: true,
                  label: _error,
                  child: const SizedBox.shrink(),
                ),
              const SizedBox(height: 12),
              const Text(
                'The page opens in this web case. Live uses tab sharing on HTTPS/localhost and automatic fallback on HTTP.',
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: _opening ? null : () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          key: const Key('web-url-open'),
          onPressed: _opening ? null : _open,
          child: Text(_opening ? 'Opening…' : 'Open URL'),
        ),
      ],
    ),
  );
}
