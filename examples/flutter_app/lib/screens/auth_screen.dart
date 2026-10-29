import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:go_router/go_router.dart';

import '../session.dart';
import '../theme.dart';

class AuthScreen extends StatefulWidget {
  const AuthScreen({
    super.key,
    required this.session,
    this.resetPassword = false,
    this.returnTo,
  });
  final DemoSession session;
  final bool resetPassword;
  final String? returnTo;

  @override
  State<AuthScreen> createState() => _AuthScreenState();
}

class _AuthScreenState extends State<AuthScreen> {
  final _form = GlobalKey<FormState>();
  final _email = TextEditingController();
  final _password = TextEditingController();
  final _errorFocus = FocusNode();
  bool _obscure = true;
  bool _busy = false;
  bool _sent = false;
  String? _error;

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    _errorFocus.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_busy) return;
    if (!_form.currentState!.validate()) {
      setState(() => _error = 'Check the highlighted fields to continue.');
      _errorFocus.requestFocus();
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    if (widget.resetPassword) {
      await Future<void>.delayed(const Duration(milliseconds: 700));
      if (!mounted) return;
      setState(() {
        _busy = false;
        _sent = true;
      });
      return;
    }
    final success = await widget.session.signIn(_email.text, _password.text);
    if (!mounted) return;
    if (success) {
      TextInput.finishAutofillContext();
    } else {
      setState(() {
        _busy = false;
        _error = 'Email or password is incorrect. Try the demo account below.';
      });
      _errorFocus.requestFocus();
    }
  }

  void _navigate(String name) => context.goNamed(
    name,
    queryParameters: {if (widget.returnTo != null) 'from': widget.returnTo!},
  );

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: LayoutBuilder(
          builder: (context, constraints) {
            final wide = constraints.maxWidth >= 960;
            final form = SingleChildScrollView(
              padding: EdgeInsets.symmetric(
                horizontal: wide ? 56 : 24,
                vertical: 32,
              ),
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 420),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      const OrbitLogo(),
                      const SizedBox(height: 48),
                      _buildForm(context),
                      const SizedBox(height: 32),
                      Text(
                        'A little less noise. A lot more focus.',
                        style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                          color: Theme.of(context).colorScheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            );
            if (!wide) return form;
            return Row(
              children: [
                const Expanded(child: _WelcomePanel()),
                Expanded(child: Center(child: form)),
              ],
            );
          },
        ),
      ),
    );
  }

  Widget _buildForm(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    if (_sent) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(Icons.mark_email_read_outlined, size: 48, color: scheme.primary),
          const SizedBox(height: 24),
          Text('You’re all set.', style: theme.textTheme.headlineLarge),
          const SizedBox(height: 12),
          const Text(
            'Reset request simulated. This demo does not send email or change your password.',
          ),
          const SizedBox(height: 24),
          FilledButton(
            onPressed: () => _navigate('login'),
            child: const Text('Back to sign in'),
          ),
        ],
      );
    }
    return AutofillGroup(
      child: Form(
        key: _form,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              widget.resetPassword
                  ? 'LET’S GET YOU BACK'
                  : 'YOUR SPACE TO FOCUS',
              style: theme.textTheme.labelMedium?.copyWith(
                color: scheme.primary,
                letterSpacing: 2,
                fontWeight: FontWeight.w700,
              ),
            ),
            const SizedBox(height: 12),
            Text(
              widget.resetPassword ? 'Forgot your password?' : 'Welcome back.',
              style: theme.textTheme.headlineLarge?.copyWith(
                fontWeight: FontWeight.w700,
                letterSpacing: -1.2,
              ),
            ),
            const SizedBox(height: 12),
            Text(
              widget.resetPassword
                  ? 'Enter your email to try the password reset flow.'
                  : 'Make room for your best work. Sign in to your workspace.',
              style: theme.textTheme.bodyLarge?.copyWith(
                color: scheme.onSurfaceVariant,
                height: 1.6,
              ),
            ),
            const SizedBox(height: 32),
            if (_error != null) ...[
              Focus(
                focusNode: _errorFocus,
                child: Semantics(
                  liveRegion: true,
                  child: Container(
                    padding: const EdgeInsets.all(16),
                    decoration: BoxDecoration(
                      color: scheme.errorContainer,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Text(
                      _error!,
                      style: TextStyle(color: scheme.onErrorContainer),
                    ),
                  ),
                ),
              ),
              const SizedBox(height: 20),
            ],
            TextFormField(
              key: const ValueKey('email-field'),
              controller: _email,
              enabled: !_busy,
              decoration: const InputDecoration(
                labelText: 'Email address',
                hintText: 'you@example.com',
                prefixIcon: Icon(Icons.mail_outline),
              ),
              keyboardType: TextInputType.emailAddress,
              autofillHints: const [
                AutofillHints.username,
                AutofillHints.email,
              ],
              textInputAction: widget.resetPassword
                  ? TextInputAction.done
                  : TextInputAction.next,
              autocorrect: false,
              validator: (value) =>
                  RegExp(
                    r'^[^\s@]+@[^\s@]+\.[^\s@]+$',
                  ).hasMatch(value?.trim() ?? '')
                  ? null
                  : 'Enter a valid email address.',
              onFieldSubmitted: widget.resetPassword ? (_) => _submit() : null,
            ),
            if (!widget.resetPassword) ...[
              const SizedBox(height: 20),
              TextFormField(
                key: const ValueKey('password-field'),
                controller: _password,
                enabled: !_busy,
                obscureText: _obscure,
                autocorrect: false,
                enableSuggestions: false,
                autofillHints: const [AutofillHints.password],
                textInputAction: TextInputAction.done,
                decoration: InputDecoration(
                  labelText: 'Password',
                  prefixIcon: const Icon(Icons.lock_outline),
                  suffixIcon: IconButton(
                    tooltip: _obscure ? 'Show password' : 'Hide password',
                    onPressed: () => setState(() => _obscure = !_obscure),
                    icon: Icon(
                      _obscure
                          ? Icons.visibility_outlined
                          : Icons.visibility_off_outlined,
                    ),
                  ),
                ),
                validator: (value) => value == null || value.isEmpty
                    ? 'Enter your password.'
                    : null,
                onFieldSubmitted: (_) => _submit(),
              ),
              Align(
                alignment: Alignment.centerRight,
                child: TextButton(
                  onPressed: _busy ? null : () => _navigate('forgot-password'),
                  child: const Text('Forgot password?'),
                ),
              ),
            ] else
              const SizedBox(height: 24),
            FilledButton(
              key: const ValueKey('submit-button'),
              onPressed: _busy ? null : _submit,
              child: _busy
                  ? Semantics(
                      label: 'Please wait',
                      child: const SizedBox(
                        height: 24,
                        width: 24,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      ),
                    )
                  : Row(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        Flexible(
                          child: Text(
                            widget.resetPassword
                                ? 'Try password reset'
                                : 'Sign in',
                          ),
                        ),
                        const SizedBox(width: 12),
                        const Icon(Icons.arrow_forward_rounded, size: 20),
                      ],
                    ),
            ),
            const SizedBox(height: 24),
            if (widget.resetPassword)
              TextButton.icon(
                onPressed: _busy ? null : () => _navigate('login'),
                icon: const Icon(Icons.arrow_back_rounded),
                label: const Text('Back to sign in'),
              )
            else ...[
              SurfaceCard(
                padding: 20,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Icon(
                          Icons.science_outlined,
                          size: 20,
                          color: scheme.primary,
                        ),
                        const SizedBox(width: 8),
                        Flexible(
                          child: Text(
                            'Just exploring?',
                            style: theme.textTheme.titleSmall,
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    const Text('A ready-to-use workspace. No account needed.'),
                    const SizedBox(height: 12),
                    const SelectableText(
                      '${DemoSession.demoEmail}\n${DemoSession.demoPassword}',
                    ),
                    const SizedBox(height: 12),
                    OutlinedButton(
                      key: const ValueKey('demo-button'),
                      onPressed: _busy
                          ? null
                          : () {
                              _email.text = DemoSession.demoEmail;
                              _password.text = DemoSession.demoPassword;
                              _submit();
                            },
                      child: const Text('Try demo workspace'),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 16),
              Text(
                'Demo mode · Your session ends when the app restarts.',
                style: theme.textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _WelcomePanel extends StatelessWidget {
  const _WelcomePanel();

  @override
  Widget build(BuildContext context) => Container(
    margin: const EdgeInsets.all(16),
    decoration: BoxDecoration(
      color: const Color(0xFF123D37),
      borderRadius: BorderRadius.circular(28),
    ),
    child: SingleChildScrollView(
      padding: const EdgeInsets.all(48),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const OrbitLogo(onDark: true),
          const SizedBox(height: 64),
          const Text(
            'Good work starts\nwith a clear mind.',
            style: TextStyle(
              fontSize: 48,
              height: 1.15,
              letterSpacing: -2,
              color: Colors.white,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 24),
          const Text(
            'Your plans, projects, and small wins.\nTogether in one calm space.',
            style: TextStyle(
              fontSize: 18,
              height: 1.6,
              color: Color(0xFFC5E3D6),
            ),
          ),
          const SizedBox(height: 48),
          Container(
            padding: const EdgeInsets.all(24),
            decoration: BoxDecoration(
              color: const Color(0xFFF2F5DC),
              borderRadius: BorderRadius.circular(20),
            ),
            child: const Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(Icons.wb_sunny_outlined, color: Color(0xFF123D37)),
                    SizedBox(width: 12),
                    Flexible(
                      child: Text(
                        'A LITTLE FOCUS GOES A LONG WAY',
                        style: TextStyle(
                          fontSize: 11,
                          color: Color(0xFF123D37),
                          letterSpacing: 1,
                        ),
                      ),
                    ),
                  ],
                ),
                SizedBox(height: 24),
                Text(
                  'One thing at a time.\nOne step closer.',
                  style: TextStyle(
                    fontSize: 28,
                    fontWeight: FontWeight.w600,
                    color: Color(0xFF123D37),
                  ),
                ),
                SizedBox(height: 24),
                LinearProgressIndicator(
                  value: 0.7,
                  color: Color(0xFF086B61),
                  backgroundColor: Color(0xFFD5DFC8),
                  minHeight: 6,
                  borderRadius: BorderRadius.all(Radius.circular(8)),
                ),
                SizedBox(height: 12),
                Text(
                  'Progress, at your own pace.',
                  style: TextStyle(color: Color(0xFF38594F)),
                ),
              ],
            ),
          ),
          const SizedBox(height: 48),
          const Text(
            'LESS FRICTION. MORE FLOW.',
            style: TextStyle(
              fontSize: 12,
              letterSpacing: 2,
              color: Color(0xFFC5E3D6),
            ),
          ),
        ],
      ),
    ),
  );
}
