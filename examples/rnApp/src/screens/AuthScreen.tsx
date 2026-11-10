import {useRef, useState} from 'react';
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  Text,
  TextInput,
  View,
  useWindowDimensions,
} from 'react-native';
import {
  DEMO_EMAIL,
  DEMO_PASSWORD,
  isValidEmail,
  simulateReset,
} from '../session';
import {styles, type Palette} from '../theme';

type Props = {
  colors: Palette;
  resetPassword: boolean;
  from: string | null;
  signIn: (email: string, password: string) => Promise<boolean>;
  go: (path: string, from?: string | null) => void;
};

export function AuthScreen({colors, resetPassword, from, signIn, go}: Props) {
  const {width} = useWindowDimensions();
  const wide = width >= 960;
  const errorRef = useRef<View>(null);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [obscure, setObscure] = useState(true);
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [emailError, setEmailError] = useState<string | null>(null);
  const [passwordError, setPasswordError] = useState<string | null>(null);

  async function submit(nextEmail = email, nextPassword = password) {
    if (busy) {
      return;
    }
    const nextEmailError = isValidEmail(nextEmail)
      ? null
      : 'Enter a valid email address.';
    const nextPasswordError = resetPassword
      ? null
      : nextPassword.length === 0
        ? 'Enter your password.'
        : null;
    if (nextEmailError || nextPasswordError) {
      setEmailError(nextEmailError);
      setPasswordError(nextPasswordError);
      setError('Check the highlighted fields to continue.');
      return;
    }
    setBusy(true);
    setError(null);
    setEmailError(null);
    setPasswordError(null);
    if (resetPassword) {
      await simulateReset();
      setBusy(false);
      setSent(true);
      return;
    }
    const success = await signIn(nextEmail, nextPassword);
    if (!success) {
      setBusy(false);
      setError('Email or password is incorrect. Try the demo account below.');
    }
  }

  const form = (
    <ScrollView
      nativeID="orbit-viewport"
      contentContainerStyle={{padding: wide ? 56 : 24, maxWidth: 420, width: '100%', alignSelf: 'center'}}>
      <Text style={[styles.logoText, {color: colors.primary}]}>orbit</Text>
      {sent ? (
        <>
          <Text style={[styles.headline, {color: colors.onSurface, marginTop: 48}]}>
            You’re all set.
          </Text>
          <Text style={[styles.lede, {color: colors.onSurface}]}>
            Reset request simulated. This demo does not send email or change your
            password.
          </Text>
          <Pressable
            testID="back-to-sign-in-button"
            nativeID="back-to-sign-in-button"
            style={[styles.button, {backgroundColor: colors.primary}]}
            onPress={() => go('/login', from)}>
            <Text style={[styles.buttonText, {color: colors.onPrimary}]}>
              Back to sign in
            </Text>
          </Pressable>
        </>
      ) : (
        <>
          <Text style={[styles.eyebrow, {color: colors.primary}]}>
            {resetPassword ? 'LET’S GET YOU BACK' : 'YOUR SPACE TO FOCUS'}
          </Text>
          <Text style={[styles.headline, {color: colors.onSurface}]}>
            {resetPassword ? 'Forgot your password?' : 'Welcome back.'}
          </Text>
          <Text style={[styles.lede, {color: colors.onSurfaceVariant}]}>
            {resetPassword
              ? 'Enter your email to try the password reset flow.'
              : 'Make room for your best work. Sign in to your workspace.'}
          </Text>
          {error ? (
            <View
              ref={errorRef}
              accessibilityLiveRegion="polite"
              style={{
                backgroundColor: colors.errorContainer,
                padding: 16,
                borderRadius: 12,
                marginBottom: 20,
              }}>
              <Text style={{color: colors.onErrorContainer}}>{error}</Text>
            </View>
          ) : null}
          <Text nativeID="email-label" style={{color: colors.onSurface, marginBottom: 6}}>
            Email address
          </Text>
          <TextInput
            testID="email-field"
            nativeID="email-field"
            accessibilityLabel="Email address"
            keyboardType="email-address"
            autoCapitalize="none"
            autoCorrect={false}
            editable={!busy}
            placeholder="you@example.com"
            value={email}
            onChangeText={setEmail}
            style={[
              styles.input,
              {
                color: colors.onSurface,
                borderColor: emailError ? colors.error : colors.outline,
                backgroundColor: colors.surfaceLowest,
              },
            ]}
          />
          {emailError ? (
            <Text style={{color: colors.error, marginBottom: 12}}>{emailError}</Text>
          ) : null}
          {resetPassword ? null : (
            <>
              <Text style={{color: colors.onSurface, marginBottom: 6}}>Password</Text>
              <View
                style={[
                  styles.row,
                  styles.input,
                  {
                    borderColor: passwordError ? colors.error : colors.outline,
                    backgroundColor: colors.surfaceLowest,
                  },
                ]}>
                <TextInput
                  testID="password-field"
                  nativeID="password-field"
                  accessibilityLabel="Password"
                  secureTextEntry={obscure}
                  autoCapitalize="none"
                  autoCorrect={false}
                  editable={!busy}
                  value={password}
                  onChangeText={setPassword}
                  onSubmitEditing={() => {
                    void submit();
                  }}
                  style={{flex: 1, color: colors.onSurface}}
                />
                <Pressable
                  testID="password-visibility-control"
                  nativeID="password-toggle-button"
                  accessibilityLabel={obscure ? 'Show password' : 'Hide password'}
                  onPress={() => setObscure(value => !value)}>
                  <Text style={{color: colors.primary}}>
                    {obscure ? 'Show password' : 'Hide password'}
                  </Text>
                </Pressable>
              </View>
              {passwordError ? (
                <Text style={{color: colors.error, marginBottom: 12}}>
                  {passwordError}
                </Text>
              ) : null}
              <Pressable
                testID="forgot-password-button"
                nativeID="forgot-password-button"
                disabled={busy}
                onPress={() => go('/forgot-password', from)}
                style={{alignSelf: 'flex-end', minHeight: 48, justifyContent: 'center'}}>
                <Text style={{color: colors.primary}}>Forgot password?</Text>
              </Pressable>
            </>
          )}
          <Pressable
            testID="submit-button"
            nativeID={resetPassword ? 'password-reset-button' : 'sign-in-button'}
            disabled={busy}
            onPress={() => {
              void submit();
            }}
            style={[
              styles.button,
              {backgroundColor: colors.primary, opacity: busy ? 0.6 : 1},
            ]}>
            {busy ? (
              <ActivityIndicator
                accessibilityLabel="Please wait"
                color={colors.onPrimary}
              />
            ) : (
              <Text style={[styles.buttonText, {color: colors.onPrimary}]}>
                {resetPassword ? 'Try password reset' : 'Sign in'}
              </Text>
            )}
          </Pressable>
          {resetPassword ? (
            <Pressable
              testID="back-to-sign-in-button"
              nativeID="back-to-sign-in-button"
              disabled={busy}
              onPress={() => go('/login', from)}
              style={{minHeight: 48, justifyContent: 'center'}}>
              <Text style={{color: colors.primary}}>Back to sign in</Text>
            </Pressable>
          ) : (
            <View
              style={[
                styles.card,
                {
                  borderColor: colors.outlineVariant,
                  backgroundColor: colors.surfaceLowest,
                  marginTop: 24,
                },
              ]}>
              <Text style={{fontWeight: '600', color: colors.onSurface}}>
                Just exploring?
              </Text>
              <Text style={{color: colors.onSurface, marginVertical: 8}}>
                A ready-to-use workspace. No account needed.
              </Text>
              <Text selectable style={{color: colors.onSurface, marginBottom: 12}}>
                {`${DEMO_EMAIL}\n${DEMO_PASSWORD}`}
              </Text>
              <Pressable
                testID="demo-button"
                nativeID="demo-button"
                disabled={busy}
                onPress={() => {
                  setEmail(DEMO_EMAIL);
                  setPassword(DEMO_PASSWORD);
                  void submit(DEMO_EMAIL, DEMO_PASSWORD);
                }}
                style={[styles.button, {borderWidth: 1, borderColor: colors.outline}]}>
                <Text style={[styles.buttonText, {color: colors.primary}]}>
                  Try demo workspace
                </Text>
              </Pressable>
            </View>
          )}
          <Text style={{color: colors.onSurfaceVariant, marginTop: 16}}>
            {resetPassword
              ? ''
              : 'Demo mode · Your session ends when the app restarts.'}
          </Text>
        </>
      )}
      <Text style={{color: colors.onSurfaceVariant, marginTop: 32}}>
        A little less noise. A lot more focus.
      </Text>
    </ScrollView>
  );

  if (!wide) {
    return <View style={[styles.flex, {backgroundColor: colors.surface}]}>{form}</View>;
  }
  return (
    <View style={[styles.flex, styles.row, {backgroundColor: colors.surface}]}>
      <View style={{flex: 1, backgroundColor: colors.welcome, margin: 16, borderRadius: 28, padding: 48}}>
        <Text style={[styles.logoText, {color: '#fff'}]}>orbit</Text>
        <Text style={{color: '#fff', fontSize: 40, fontWeight: '600', marginTop: 48}}>
          {'Good work starts\nwith a clear mind.'}
        </Text>
        <Text style={{color: colors.welcomeMuted, fontSize: 18, marginTop: 24}}>
          {'Your plans, projects, and small wins.\nTogether in one calm space.'}
        </Text>
      </View>
      <View style={styles.flex}>{form}</View>
    </View>
  );
}
