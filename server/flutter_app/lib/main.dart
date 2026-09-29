import 'package:flutter/material.dart';

import 'companion_home.dart';

void main() => runApp(const MobileMcpCompanion());

class MobileMcpCompanion extends StatelessWidget {
  const MobileMcpCompanion({super.key});

  @override
  Widget build(BuildContext context) {
    const colors = ColorScheme.dark(
      primary: Color(0xFF0F172A),
      secondary: Color(0xFF1E293B),
      surface: Color(0xFF0E1223),
      onSurface: Color(0xFFF8FAFC),
      onPrimary: Color(0xFFFFFFFF),
      onSecondary: Color(0xFFFFFFFF),
      error: Color(0xFFDC2626),
    );
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'Device Bot Companion',
      theme: ThemeData(
        brightness: Brightness.dark,
        colorScheme: colors,
        scaffoldBackgroundColor: const Color(0xFF020617),
        useMaterial3: true,
        cardTheme: const CardThemeData(
          color: Color(0xFF0E1223),
          margin: EdgeInsets.zero,
        ),
        inputDecorationTheme: const InputDecorationTheme(
          filled: true,
          fillColor: Color(0xFF1A1E2F),
          border: OutlineInputBorder(),
        ),
      ),
      home: const CompanionHome(),
    );
  }
}
