import {useState, type ReactNode} from 'react';
import {
  PixelRatio,
  Pressable,
  ScrollView,
  Text,
  View,
  useWindowDimensions,
} from 'react-native';
import {DEMO_EMAIL, TASKS} from '../session';
import {styles, type Palette} from '../theme';

type ShellProps = {
  colors: Palette;
  account: boolean;
  signOut: () => void;
  go: (path: string) => void;
  children: ReactNode;
};

export function WorkspaceShell({colors, account, signOut, go, children}: ShellProps) {
  const {width} = useWindowDimensions();
  const wide = width >= 800;
  return (
    <View style={[styles.flex, {backgroundColor: colors.surface}]}>
      <View
        style={[
          styles.row,
          {minHeight: 80, paddingHorizontal: 16, justifyContent: 'space-between'},
        ]}>
        <Text style={[styles.logoText, {color: colors.primary}]}>orbit</Text>
        <Pressable
          testID="account-avatar-control"
          nativeID="account-avatar-button"
          accessibilityLabel="Your account"
          onPress={() => go('/account')}
          style={[styles.avatar, {backgroundColor: colors.primaryContainer}]}>
          <Text style={{color: colors.onPrimaryContainer, fontWeight: '700'}}>AL</Text>
        </Pressable>
      </View>
      <View style={styles.flex}>
        <View style={[styles.flex, {flexDirection: 'row'}]}>
          {wide ? (
            <View
              style={{
                width: 88,
                borderRightWidth: 1,
                borderRightColor: colors.outlineVariant,
                alignItems: 'center',
                paddingBottom: 24,
              }}>
              <NavItem
                colors={colors}
                label="Overview"
                active={!account}
                onPress={() => go('/home')}
              />
              <NavItem
                colors={colors}
                label="Account"
                active={account}
                onPress={() => go('/account')}
              />
              <View style={styles.flex} />
              <Pressable
                testID="rail-sign-out-control"
                nativeID="rail-sign-out-button"
                accessibilityLabel="Sign out"
                onPress={signOut}
                style={{minHeight: 48, justifyContent: 'center'}}>
                <Text style={{color: colors.primary}}>Sign out</Text>
              </Pressable>
            </View>
          ) : null}
          {children}
        </View>
      </View>
      {wide ? null : (
        <View
          style={[
            styles.row,
            {borderTopWidth: 1, borderTopColor: colors.outlineVariant},
          ]}>
          <NavItem
            colors={colors}
            label="Overview"
            active={!account}
            onPress={() => go('/home')}
            grow
          />
          <NavItem
            colors={colors}
            label="Account"
            active={account}
            onPress={() => go('/account')}
            grow
          />
        </View>
      )}
    </View>
  );
}

function NavItem({
  colors,
  label,
  active,
  onPress,
  grow,
}: {
  colors: Palette;
  label: string;
  active: boolean;
  onPress: () => void;
  grow?: boolean;
}) {
  return (
    <Pressable
      onPress={onPress}
      style={{
        minHeight: 56,
        alignItems: 'center',
        justifyContent: 'center',
        paddingHorizontal: 8,
        flex: grow ? 1 : undefined,
      }}>
      <Text style={{color: active ? colors.primary : colors.onSurfaceVariant}}>
        {label}
      </Text>
    </Pressable>
  );
}

export function HomeScreen({
  colors,
  go,
}: {
  colors: Palette;
  go: (path: string) => void;
}) {
  const {width} = useWindowDimensions();
  const [completed, setCompleted] = useState<Set<number>>(new Set([0]));
  const stack = width < 640 + (width >= 800 ? 88 : 0) || PixelRatio.getFontScale() * 16 > 24;
  const padding = width >= 800 ? 40 : 24;

  function toggle(index: number) {
    setCompleted(current => {
      const next = new Set(current);
      if (next.has(index)) {
        next.delete(index);
      } else {
        next.add(index);
      }
      return next;
    });
  }

  return (
    <ScrollView
      nativeID="orbit-viewport"
      style={styles.flex}
      contentContainerStyle={{padding, maxWidth: 1040, width: '100%', alignSelf: 'center'}}>
      <Text style={[styles.eyebrow, {color: colors.primary}]}>YOUR WORKSPACE</Text>
      <Text style={[styles.headline, {color: colors.onSurface}]}>A fresh start, Alex.</Text>
      <Text style={[styles.lede, {color: colors.onSurfaceVariant}]}>
        Let’s make a little progress on what matters.
      </Text>
      <View
        style={{
          backgroundColor: colors.primaryContainer,
          borderRadius: 24,
          padding: 28,
          marginBottom: 24,
        }}>
        <Text style={{fontSize: 24, fontWeight: '600', color: colors.onPrimaryContainer}}>
          Find your flow.
        </Text>
        <Text style={{color: colors.onPrimaryContainer, marginVertical: 12, lineHeight: 24}}>
          {'Big ideas begin with small, intentional steps.\nYour next one is right here.'}
        </Text>
        <Pressable
          testID="view-profile-button"
          nativeID="view-profile-button"
          onPress={() => go('/account')}
          style={[styles.button, {backgroundColor: colors.primary}]}>
          <Text style={[styles.buttonText, {color: colors.onPrimary}]}>
            View your profile
          </Text>
        </Pressable>
      </View>
      <View style={{flexDirection: stack ? 'column' : 'row', gap: 16, marginBottom: 24}}>
        <Stat colors={colors} value={`${completed.size}/3`} label="Small wins" />
        <Stat colors={colors} value="20 min" label="Time for yourself" />
        <Stat colors={colors} value="Today" label="A good day to begin" />
      </View>
      <View
        style={[
          styles.card,
          {borderColor: colors.outlineVariant, backgroundColor: colors.surfaceLowest},
        ]}>
        <Text style={{fontSize: 22, fontWeight: '600', color: colors.onSurface}}>
          A little plan for today
        </Text>
        <Text style={{color: colors.onSurface, marginBottom: 12}}>
          Check things off. Enjoy the small wins.
        </Text>
        {TASKS.map((task, index) => {
          const done = completed.has(index);
          return (
            <Pressable
              key={task.title}
              testID={`task-${index + 1}-checkbox`}
              nativeID={`task-${index + 1}-checkbox`}
              onPress={() => toggle(index)}
              style={styles.row}>
              <View
                style={{
                  width: 22,
                  height: 22,
                  borderRadius: 4,
                  borderWidth: 2,
                  borderColor: colors.primary,
                  marginRight: 12,
                  backgroundColor: done ? colors.primary : 'transparent',
                }}
              />
              <View style={{flex: 1, paddingVertical: 8}}>
                <Text
                  style={{
                    color: colors.onSurface,
                    textDecorationLine: done ? 'line-through' : 'none',
                  }}>
                  {task.title}
                </Text>
                <Text style={{color: colors.onSurfaceVariant}}>{task.detail}</Text>
              </View>
            </Pressable>
          );
        })}
        <Text accessibilityLabel={`${completed.size} of 3 tasks complete`} style={{marginTop: 8}}>
          {`${completed.size} of 3 tasks complete`}
        </Text>
      </View>
      <Text style={{color: colors.onSurfaceVariant}}>Your pace. Your space. Your orbit.</Text>
    </ScrollView>
  );
}

function Stat({
  colors,
  value,
  label,
}: {
  colors: Palette;
  value: string;
  label: string;
}) {
  return (
    <View
      style={[
        styles.card,
        {flex: 1, borderColor: colors.outlineVariant, backgroundColor: colors.surfaceLowest},
      ]}>
      <Text style={{fontSize: 24, fontWeight: '600', color: colors.onSurface}}>{value}</Text>
      <Text style={{color: colors.onSurface}}>{label}</Text>
    </View>
  );
}

export function AccountScreen({
  colors,
  signOut,
}: {
  colors: Palette;
  signOut: () => void;
}) {
  const {width} = useWindowDimensions();
  const padding = width >= 800 ? 40 : 24;
  return (
    <ScrollView
      nativeID="orbit-viewport"
      style={styles.flex}
      contentContainerStyle={{padding, maxWidth: 1040, width: '100%', alignSelf: 'center'}}>
      <Text style={[styles.headline, {color: colors.onSurface}]}>Your account</Text>
      <Text style={[styles.lede, {color: colors.onSurface}]}>
        A familiar face in your own little space.
      </Text>
      <View
        style={[
          styles.card,
          {borderColor: colors.outlineVariant, backgroundColor: colors.surfaceLowest},
        ]}>
        <View
          style={[
            styles.avatar,
            {width: 72, height: 72, borderRadius: 36, backgroundColor: colors.primaryContainer},
          ]}>
          <Text style={{fontSize: 24, color: colors.onPrimaryContainer}}>AL</Text>
        </View>
        <Text style={{fontSize: 24, marginTop: 16, color: colors.onSurface}}>Alex Lane</Text>
        <Text selectable style={{color: colors.onSurface, marginVertical: 8}}>
          {DEMO_EMAIL}
        </Text>
        <Text style={{fontWeight: '600', color: colors.onSurface, marginTop: 16}}>
          Your personal workspace
        </Text>
        <Text style={{color: colors.onSurface, marginVertical: 8}}>
          You’re exploring a demo account. Changes and your sign-in session are kept only
          while the app is running.
        </Text>
        <Pressable
          testID="sign-out-button"
          nativeID="sign-out-button"
          onPress={signOut}
          style={[styles.button, {borderWidth: 1, borderColor: colors.outline}]}>
          <Text style={[styles.buttonText, {color: colors.primary}]}>Sign out</Text>
        </Pressable>
      </View>
    </ScrollView>
  );
}

export function NotFound({
  colors,
  go,
}: {
  colors: Palette;
  go: (path: string) => void;
}) {
  return (
    <View style={[styles.flex, {alignItems: 'center', justifyContent: 'center', padding: 24}]}>
      <Text style={{color: colors.onSurface, marginBottom: 16}}>
        This page is outside your orbit.
      </Text>
      <Pressable
        style={[styles.button, {backgroundColor: colors.primary}]}
        onPress={() => go('/home')}>
        <Text style={[styles.buttonText, {color: colors.onPrimary}]}>Back to home</Text>
      </Pressable>
    </View>
  );
}
