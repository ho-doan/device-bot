import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:mobile_mcp_companion/core/models/admin_models.dart';

const _unset = Object();

class LaunchAppState {
  const LaunchAppState({
    this.apps = const [],
    this.loading = true,
    this.error,
    this.query = '',
    this.identifier = '',
    this.selectedId,
  });
  final List<InstalledApp> apps;
  final bool loading;
  final Object? error;
  final String query;
  final String identifier;
  final String? selectedId;

  LaunchAppState copyWith({
    List<InstalledApp>? apps,
    bool? loading,
    Object? error = _unset,
    String? query,
    String? identifier,
    Object? selectedId = _unset,
  }) => LaunchAppState(
    apps: apps ?? this.apps,
    loading: loading ?? this.loading,
    error: identical(error, _unset) ? this.error : error,
    query: query ?? this.query,
    identifier: identifier ?? this.identifier,
    selectedId: identical(selectedId, _unset)
        ? this.selectedId
        : selectedId as String?,
  );
}

class LaunchAppCubit extends Cubit<LaunchAppState> {
  LaunchAppCubit() : super(const LaunchAppState());
  Future<void> load(Future<List<InstalledApp>> result) async {
    try {
      final apps = await result;
      if (!isClosed) {
        emit(state.copyWith(apps: List.unmodifiable(apps), loading: false));
      }
    } catch (error) {
      if (!isClosed) emit(state.copyWith(error: error, loading: false));
    }
  }

  void search(String value) => emit(
    state.copyWith(
      query: value,
      selectedId: null,
      identifier: state.selectedId == null ? state.identifier : '',
    ),
  );
  void choose(InstalledApp app) => emit(
    state.copyWith(selectedId: app.identifier, identifier: app.identifier),
  );
  void enterIdentifier(String value) =>
      emit(state.copyWith(identifier: value, selectedId: null));

  List<InstalledApp> get suggestions {
    final query = state.query.trim().toLowerCase();
    final terms = query.split(RegExp(r'\s+'));
    final matches = state.apps.where((app) {
      final searchable = '${app.name} ${app.identifier}'.toLowerCase();
      return terms.every(searchable.contains);
    }).toList();
    int rank(InstalledApp app) {
      final name = app.name.toLowerCase(), id = app.identifier.toLowerCase();
      if (name == query || id == query) return 0;
      if (name.startsWith(query)) return 1;
      if (id.startsWith(query)) return 2;
      return 3;
    }

    matches.sort((a, b) {
      final order = rank(a).compareTo(rank(b));
      return order != 0
          ? order
          : a.name.toLowerCase().compareTo(b.name.toLowerCase());
    });
    return matches;
  }
}
