import {useCallback, useMemo, useState} from 'react';
import {StatusBar, useColorScheme} from 'react-native';
import {SafeAreaProvider, SafeAreaView} from 'react-native-safe-area-context';
import {AuthScreen} from './src/screens/AuthScreen';
import {
  AccountScreen,
  HomeScreen,
  NotFound,
  WorkspaceShell,
} from './src/screens/Workspace';
import {
  authenticate,
  KNOWN_PATHS,
  resolveLocation,
  type OrbitLocation,
} from './src/session';
import {dark, light} from './src/theme';

export default function App({initialPath = '/'}: {initialPath?: string}) {
  const isDark = useColorScheme() === 'dark';
  const colors = isDark ? dark : light;
  const [signedIn, setSignedIn] = useState(false);
  const [location, setLocation] = useState<OrbitLocation>(() =>
    resolveLocation(initialPath, null, false),
  );

  const go = useCallback(
    (path: string, from?: string | null) => {
      setLocation(current =>
        resolveLocation(path, from === undefined ? current.from : from, signedIn),
      );
    },
    [signedIn],
  );

  const signIn = useCallback(async (email: string, password: string) => {
    const success = await authenticate(email, password);
    if (success) {
      setSignedIn(true);
      setLocation(current => resolveLocation(current.path, current.from, true));
    }
    return success;
  }, []);

  const signOut = useCallback(() => {
    setSignedIn(false);
    setLocation(current => resolveLocation(current.path, current.from, false));
  }, []);

  const screen = useMemo(() => {
    if (!KNOWN_PATHS.has(location.path)) {
      return <NotFound colors={colors} go={go} />;
    }
    if (location.path === '/login' || location.path === '/forgot-password') {
      return (
        <AuthScreen
          colors={colors}
          resetPassword={location.path === '/forgot-password'}
          from={location.from}
          signIn={signIn}
          go={go}
        />
      );
    }
    const account = location.path === '/account';
    return (
      <WorkspaceShell colors={colors} account={account} signOut={signOut} go={go}>
        {account ? (
          <AccountScreen colors={colors} signOut={signOut} />
        ) : (
          <HomeScreen colors={colors} go={go} />
        )}
      </WorkspaceShell>
    );
  }, [colors, go, location.from, location.path, signIn, signOut]);

  return (
    <SafeAreaProvider>
      <StatusBar barStyle={isDark ? 'light-content' : 'dark-content'} />
      <SafeAreaView style={{flex: 1, backgroundColor: colors.surface}}>
        {screen}
      </SafeAreaView>
    </SafeAreaProvider>
  );
}
