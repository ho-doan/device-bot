import 'package:flutter/foundation.dart';

/// In-memory demo authentication. Replace with a repository for a real backend.
class DemoSession extends ChangeNotifier {
  static const demoEmail = 'alex@orbit.demo';
  static const demoPassword = 'Orbit123!';

  bool get signedIn => _signedIn;
  bool _signedIn = false;

  Future<bool> signIn(String email, String password) async {
    await Future<void>.delayed(const Duration(milliseconds: 700));
    if (email.trim().toLowerCase() != demoEmail || password != demoPassword) {
      return false;
    }
    _signedIn = true;
    notifyListeners();
    return true;
  }

  void signOut() {
    _signedIn = false;
    notifyListeners();
  }
}
