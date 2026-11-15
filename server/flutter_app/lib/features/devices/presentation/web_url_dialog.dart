import 'dart:async';
import 'package:flutter_bloc/flutter_bloc.dart';
import '../cubit/web_url_cubit.dart';
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
  late final _cubit = WebUrlCubit(widget.onOpen);
  bool get _opening => _cubit.state.opening;
  String? get _error => _cubit.state.error;
  @override
  void dispose() {
    unawaited(_cubit.close());
    _url.dispose();
    super.dispose();
  }

  Future<void> _open() async {
    final opened = await _cubit.open(_url.text);
    if (mounted && opened) Navigator.of(context).pop(true);
  }

  @override
  Widget build(BuildContext context) => BlocBuilder<WebUrlCubit, WebUrlState>(
    bloc: _cubit,
    builder: (context, state) => _buildDialog(context),
  );

  Widget _buildDialog(BuildContext context) => PopScope(
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
