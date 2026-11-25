import React, {useEffect, useRef, useState} from 'react';
import {
  BackHandler,
  Pressable,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
} from 'react-native';
import {
  SafeAreaProvider,
  initialWindowMetrics,
  useSafeAreaInsets,
} from 'react-native-safe-area-context';
import GameSurface, {
  Commands,
  type NativeProps,
} from './specs/ZombieboxSurfaceNativeComponent';

const MAX_HEALTH = 5;
const SEED = '42';

type Hud = Parameters<NonNullable<NativeProps['onHudChanged']>>[0]['nativeEvent'];
type Menu = 'start' | 'paused' | 'won' | 'dead';

const menuContent = {
  start: {
    title: 'Zombiebox',
    description: 'Find the pistol inside the house and clear the block.',
    action: 'Enter the block',
  },
  paused: {
    title: 'Paused',
    description: null,
    action: 'Resume',
  },
  won: {
    title: 'Block cleared',
    description: 'All three infected are down.',
    action: 'Play again',
  },
  dead: {
    title: 'Survivor lost',
    description: 'The infected caught you.',
    action: 'Try again',
  },
};

function Game() {
  const insets = useSafeAreaInsets();
  const {height, width} = useWindowDimensions();
  const surface = useRef<React.ComponentRef<typeof GameSurface>>(null);
  const [run, setRun] = useState(1);
  const [menu, setMenu] = useState<Menu | null>('start');
  const [hud, setHud] = useState<Hud | null>(null);
  const activeMenu: Menu | null =
    menu ?? (hud?.won ? 'won' : hud?.dead ? 'dead' : null);
  const compact = height - insets.top - insets.bottom < 340;
  const menuWidth = Math.min(356, Math.max(260, Math.floor(width * 0.49)));

  useEffect(() => {
    const back = BackHandler.addEventListener('hardwareBackPress', () => {
      if (activeMenu === null || activeMenu === 'paused') {
        setMenu(activeMenu === null ? 'paused' : null);
        return true;
      }
      return false;
    });
    return () => back.remove();
  }, [activeMenu]);

  const restart = () => {
    setHud(null);
    setRun(previous => previous + 1);
    setMenu(null);
  };

  const use = () => {
    if (surface.current) Commands.requestUse(surface.current);
  };

  const content = activeMenu ? menuContent[activeMenu] : null;
  const health = hud?.health ?? MAX_HEALTH;
  const action = hud?.nearLoot ? 'Search' : hud?.armed ? 'Fire' : 'Use';
  const actionLabel = hud?.nearLoot
    ? 'Search the cache'
    : hud?.armed ? 'Fire pistol' : 'Use';
  const outOfAmmo = !!hud?.armed && !hud.nearLoot && hud.supplies === 0;
  const objective = hud?.nearLoot
    ? 'Search the cache'
    : hud?.armed
      ? 'Clear the block'
      : 'Find the pistol inside the house';

  return (
    <View style={styles.screen}>
      <GameSurface
        ref={surface}
        style={StyleSheet.absoluteFill}
        runId={String(run)}
        seed={SEED}
        paused={activeMenu !== null}
        onHudChanged={event => setHud(event.nativeEvent)}
      />
      {activeMenu === null && (
        <>
          <View
            pointerEvents="box-none"
            style={[
              styles.top,
              {top: insets.top + 12, left: insets.left + 16, right: insets.right + 16},
            ]}>
            <View pointerEvents="none" style={styles.status}>
              <View accessible accessibilityLabel={`Health ${health} out of ${MAX_HEALTH}`}>
                <Text style={styles.statusLabel}>HEALTH</Text>
                <View style={styles.health}>
                  {Array.from({length: MAX_HEALTH}, (_, index) => (
                    <View key={index} style={[styles.healthBar, index < health && styles.healthFull]} />
                  ))}
                </View>
              </View>
              <View style={styles.weapon}>
                <Text style={styles.statusLabel}>{hud?.armed ? 'PISTOL' : 'WEAPON'}</Text>
                <Text style={styles.statusValue}>{hud?.armed ? `${hud.supplies} rounds` : 'Unarmed'}</Text>
              </View>
            </View>

            <View pointerEvents="none" style={styles.objective}>
              <View style={styles.objectiveCard}>
                <Text style={styles.statusLabel}>OBJECTIVE</Text>
                <Text style={styles.objectiveText}>{objective}</Text>
              </View>
            </View>

            <View pointerEvents="box-none" style={styles.right}>
              <View
                accessible
                accessibilityLabel={`${hud?.zombies ?? 3} infected remaining`}
                pointerEvents="none"
                style={styles.threats}>
                <Text style={styles.statusLabel}>INFECTED</Text>
                <Text style={styles.threatCount}>{hud?.zombies ?? 3}</Text>
              </View>
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Pause game"
                onPress={() => setMenu('paused')}
                style={({pressed}) => [styles.pause, pressed && styles.controlPressed]}>
                <View style={styles.pauseBar} />
                <View style={styles.pauseBar} />
              </Pressable>
            </View>
          </View>

          <View pointerEvents="box-none" style={[styles.bottom, {bottom: insets.bottom + (compact ? 10 : 20)}]}>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={outOfAmmo ? 'Out of rounds' : actionLabel}
              disabled={outOfAmmo}
              onPress={use}
              style={({pressed}) => [
                styles.action,
                hud?.armed && !outOfAmmo && styles.actionArmed,
                hud?.nearLoot && styles.actionReady,
                outOfAmmo && styles.actionDisabled,
                pressed && styles.controlPressed,
              ]}>
              <Text style={[styles.actionText, hud?.nearLoot && styles.actionReadyText]}>
                {outOfAmmo ? 'Out of rounds' : action}
              </Text>
            </Pressable>
          </View>
        </>
      )}

      {content && activeMenu && (
        <View
          accessibilityViewIsModal
          style={[
            styles.scrim,
            {
              paddingLeft: insets.left + 20,
              paddingRight: insets.right + 20,
              paddingTop: insets.top + 14,
              paddingBottom: insets.bottom + 14,
            },
          ]}>
          <View style={[styles.menu, {width: menuWidth}, compact && styles.menuCompact]}>
            <Text style={styles.location}>BIRCH STREET</Text>
            <Text
              accessibilityRole="header"
              style={[
                styles.title,
                activeMenu === 'start' && styles.gameTitle,
                compact && styles.titleCompact,
              ]}>
              {content.title}
            </Text>
            {content.description && <Text style={styles.description}>{content.description}</Text>}
            {activeMenu === 'start' && (
              <View style={styles.instructions}>
                <Text style={styles.instructionText}>Left stick to move · Right stick to aim</Text>
                <Text style={styles.instructionText}>Tap the scene or action button to search or fire.</Text>
              </View>
            )}
            <View style={styles.menuActions}>
              <Pressable
                accessibilityRole="button"
                onPress={activeMenu === 'won' || activeMenu === 'dead' ? restart : () => setMenu(null)}
                style={({pressed}) => [styles.primary, pressed && styles.controlPressed]}>
                <Text style={styles.primaryText}>{content.action}</Text>
              </Pressable>
              {activeMenu === 'paused' && (
                <Pressable
                  accessibilityRole="button"
                  onPress={restart}
                  style={({pressed}) => [styles.secondary, pressed && styles.controlPressed]}>
                  <Text style={styles.secondaryText}>Restart run</Text>
                </Pressable>
              )}
            </View>
          </View>
        </View>
      )}
    </View>
  );
}

export default function App() {
  return (
    <SafeAreaProvider initialMetrics={initialWindowMetrics}>
      <Game />
    </SafeAreaProvider>
  );
}

const amber = '#E1AE63';
const bone = '#F1EAE0';
const muted = '#BEBBB3';
const ink = '#15191D';
const glass = '#171D22F2';
const outline = '#465159';

const styles = StyleSheet.create({
  screen: {flex: 1, backgroundColor: ink},
  top: {position: 'absolute', flexDirection: 'row', alignItems: 'flex-start', gap: 8},
  status: {
    flexDirection: 'row', alignItems: 'center', gap: 10, paddingHorizontal: 10,
    minHeight: 44, backgroundColor: glass, borderWidth: 1, borderColor: outline,
  },
  statusLabel: {
    fontFamily: 'monospace', fontSize: 10, fontWeight: '700', letterSpacing: 0.6, color: muted,
  },
  health: {flexDirection: 'row', gap: 3, marginTop: 4},
  healthBar: {width: 9, height: 6, backgroundColor: '#464742'},
  healthFull: {backgroundColor: amber},
  weapon: {borderLeftWidth: 1, borderColor: outline, paddingLeft: 10},
  statusValue: {fontSize: 12, color: bone, fontWeight: '700'},
  objective: {flex: 1, alignItems: 'center'},
  objectiveCard: {
    alignItems: 'center', justifyContent: 'center', minHeight: 44, paddingHorizontal: 12,
    paddingVertical: 3, borderBottomWidth: 2, borderColor: amber, backgroundColor: glass,
  },
  objectiveText: {color: bone, fontSize: 12, fontWeight: '700', textAlign: 'center'},
  right: {flexDirection: 'row', alignItems: 'center', gap: 6},
  threats: {
    paddingHorizontal: 10, paddingVertical: 3, justifyContent: 'center', minHeight: 44,
    borderWidth: 1, borderColor: outline, backgroundColor: glass,
  },
  threatCount: {color: bone, fontFamily: 'monospace', fontSize: 15, fontWeight: '700'},
  pause: {
    minWidth: 44, minHeight: 44, backgroundColor: glass, borderWidth: 1, borderColor: outline,
    flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 4,
  },
  pauseBar: {width: 3, height: 12, backgroundColor: bone},
  bottom: {position: 'absolute', left: 0, right: 0, alignItems: 'center'},
  action: {
    minHeight: 44, minWidth: 104, borderWidth: 1, borderColor: outline,
    justifyContent: 'center', alignItems: 'center', paddingHorizontal: 12, backgroundColor: glass,
  },
  actionText: {fontSize: 14, fontWeight: '800', letterSpacing: 0.4, color: bone},
  actionArmed: {borderColor: amber},
  actionReady: {backgroundColor: amber, borderColor: amber},
  actionReadyText: {color: ink},
  actionDisabled: {opacity: 0.75},
  controlPressed: {opacity: 0.72},
  scrim: {
    ...StyleSheet.absoluteFill,
    backgroundColor: '#10131765',
    justifyContent: 'center',
    alignItems: 'flex-start',
  },
  menu: {
    maxWidth: '100%', backgroundColor: '#171C21FA', borderWidth: 1, borderColor: outline,
    borderLeftWidth: 3, borderLeftColor: amber, paddingHorizontal: 22, paddingVertical: 19, gap: 5,
  },
  menuCompact: {paddingHorizontal: 16, paddingVertical: 12, gap: 4},
  location: {
    color: amber, fontFamily: 'monospace', fontSize: 11, fontWeight: '700', letterSpacing: 2,
  },
  title: {
    color: bone, fontFamily: 'sans-serif-condensed', fontSize: 42, lineHeight: 47,
    fontWeight: '900', letterSpacing: -0.6,
  },
  gameTitle: {fontSize: 53, lineHeight: 56, textTransform: 'uppercase'},
  titleCompact: {fontSize: 36, lineHeight: 40},
  description: {color: bone, fontSize: 14, lineHeight: 19},
  instructions: {
    borderTopWidth: 1, borderColor: outline, marginTop: 6, paddingTop: 7, gap: 2,
  },
  instructionText: {color: muted, fontSize: 12, lineHeight: 15},
  menuActions: {flexDirection: 'row', flexWrap: 'wrap', gap: 8, marginTop: 7},
  primary: {
    alignItems: 'center', justifyContent: 'center', minHeight: 44, paddingHorizontal: 16,
    backgroundColor: amber,
  },
  primaryText: {fontSize: 14, color: ink, fontWeight: '900'},
  secondary: {
    alignItems: 'center', justifyContent: 'center', minHeight: 44, paddingHorizontal: 14,
    borderColor: outline, borderWidth: 1,
  },
  secondaryText: {fontSize: 14, color: bone, fontWeight: '700'},
});
