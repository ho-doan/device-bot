import 'package:flutter_bloc/flutter_bloc.dart';

class WebUrlState {
  const WebUrlState({this.opening = false, this.error});
  final bool opening;
  final String? error;
}

class WebUrlCubit extends Cubit<WebUrlState> {
  WebUrlCubit(this._open) : super(const WebUrlState());
  final Future<void> Function(String) _open;

  Future<bool> open(String text) async {
    if (isClosed || state.opening) return false;
    final value = text.trim();
    final uri = Uri.tryParse(value);
    if (uri == null ||
        !['https', 'http'].contains(uri.scheme) ||
        uri.host.isEmpty ||
        value.contains(RegExp(r'\s'))) {
      emit(
        const WebUrlState(
          error:
              'Enter a complete HTTP or HTTPS URL, such as http://localhost:3000.',
        ),
      );
      return false;
    }
    emit(const WebUrlState(opening: true));
    try {
      await _open(value);
      if (isClosed) return false;
      emit(const WebUrlState());
      return true;
    } catch (error) {
      if (!isClosed) emit(WebUrlState(error: 'Could not open URL: $error'));
      return false;
    }
  }
}
