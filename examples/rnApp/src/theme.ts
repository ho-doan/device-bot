import {StyleSheet} from 'react-native';

export const light = {
  primary: '#086B61',
  onPrimary: '#FFFFFF',
  primaryContainer: '#9FF2E5',
  onPrimaryContainer: '#00201C',
  surface: '#F8FAF8',
  onSurface: '#171D1C',
  onSurfaceVariant: '#3F4947',
  outline: '#6F7977',
  outlineVariant: '#BEC9C6',
  surfaceLowest: '#FFFFFF',
  error: '#BA1A1A',
  errorContainer: '#FFDAD6',
  onErrorContainer: '#410002',
  welcome: '#123D37',
  welcomeMuted: '#C5E3D6',
  welcomeCard: '#F2F5DC',
};

export const dark = {
  ...light,
  primary: '#80DED0',
  onPrimary: '#003731',
  primaryContainer: '#005048',
  onPrimaryContainer: '#9FF2E5',
  surface: '#101F21',
  onSurface: '#DEE4E2',
  onSurfaceVariant: '#BEC9C6',
  outline: '#899390',
  outlineVariant: '#3F4947',
  surfaceLowest: '#0A1517',
  error: '#FFB4AB',
  errorContainer: '#93000A',
  onErrorContainer: '#FFDAD6',
};

export type Palette = typeof light;

export const styles = StyleSheet.create({
  flex: {flex: 1},
  row: {flexDirection: 'row', alignItems: 'center'},
  logoText: {fontSize: 30, fontWeight: '800', letterSpacing: -1.5},
  eyebrow: {
    letterSpacing: 2,
    fontWeight: '700',
    fontSize: 12,
    marginTop: 24,
    marginBottom: 12,
  },
  headline: {
    fontSize: 32,
    fontWeight: '700',
    letterSpacing: -1.2,
    marginBottom: 12,
  },
  lede: {fontSize: 16, lineHeight: 24, marginBottom: 24},
  card: {borderRadius: 20, borderWidth: 1, padding: 24, marginBottom: 16},
  input: {
    minHeight: 56,
    borderWidth: 1,
    borderRadius: 12,
    paddingHorizontal: 16,
    marginBottom: 8,
  },
  button: {
    minHeight: 56,
    borderRadius: 12,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 24,
    marginTop: 8,
  },
  buttonText: {fontSize: 16, fontWeight: '600'},
  avatar: {
    width: 40,
    height: 40,
    borderRadius: 20,
    alignItems: 'center',
    justifyContent: 'center',
  },
});
