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
    description: 'Find the pistol inside the house and clear the residential block.',
    action: 'Enter the block',
  },
  paused: {
    title: 'Paused',
    description: 'Resume when you’re ready.',
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
  const {height} = useWindowDimensions();
  const surface = useRef<React.ComponentRef<typeof GameSurface>>(null);
  const [run, setRun] = useState(1);
  const [menu, setMenu] = useState<Menu | null>('start');
  const [hud, setHud] = useState<Hud | null>(null);
  const activeMenu: Menu | null = menu ?? (hud?.won ? 'won' : hud?.dead ? 'dead' : null);
  const compact = height < 340;

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
  const actionLabel = hud?.nearLoot ? 'Search the cache' : hud?.armed ? 'Fire pistol' : 'Use';
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
            style={[styles.top, {top: insets.top + 14, left: insets.left + 18, right: insets.right + 18}]}>
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
              <Text style={styles.objectiveText}>{objective}</Text>
            </View>

            <View pointerEvents="box-none" style={styles.right}>
              <View pointerEvents="none" style={styles.threats}>
                <Text style={styles.statusLabel}>INFECTED</Text>
                <Text style={styles.statusValue}>{hud?.zombies ?? 3} left</Text>
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
              style={({pressed}) => [styles.action, hud?.nearLoot && styles.actionReady, outOfAmmo && styles.actionDisabled, pressed && styles.controlPressed]}>
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
          style={[styles.scrim, {paddingLeft: insets.left + 20, paddingRight: insets.right + 20, paddingTop: insets.top + 14, paddingBottom: insets.bottom + 14}]}>
          <View style={[styles.menu, compact && styles.menuCompact]}>
            <Text accessibilityRole="header" style={[styles.title, compact && styles.titleCompact]}>{content.title}</Text>
            <Text style={styles.description}>{content.description}</Text>
            {activeMenu === 'start' && (
              <Text style={styles.instructions}>
                Left stick to move. Right stick to aim. Tap the scene or action button to search or fire.
              </Text>
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

const amber = '#E5B96D';
const cream = '#F3E4C4';
const charcoal = '#151C19';
const glass = '#151C19E8';
const outline = '#E5B96D55';

const styles = StyleSheet.create({
  screen: {flex: 1, backgroundColor: charcoal},
  top: {position: 'absolute', flexDirection: 'row', alignItems: 'flex-start', gap: 8},
  status: {
    flexDirection: 'row', alignItems: 'center', gap: 11, paddingHorizontal: 10,
    minHeight: 44, backgroundColor: glass, borderWidth: 1, borderColor: outline, borderRadius: 7,
  },
  statusLabel: {fontFamily: 'monospace', fontSize: 9, letterSpacing: 0.4, color: amber},
  health: {flexDirection: 'row', gap: 3, marginTop: 4},
  healthBar: {width: 10, height: 6, borderRadius: 1, backgroundColor: '#665242'},
  healthFull: {backgroundColor: '#D78257'},
  weapon: {borderLeftWidth: 1, borderColor: outline, paddingLeft: 10},
  statusValue: {fontSize: 11, color: cream, fontWeight: '600'},
  objective: {flex: 1, alignItems: 'center', paddingTop: 4},
  objectiveText: {
    color: cream, fontSize: 11, fontWeight: '700', textAlign: 'center',
    backgroundColor: glass, paddingHorizontal: 9, paddingVertical: 6, borderRadius: 4,
    overflow: 'hidden',
  },
  right: {flexDirection: 'row', alignItems: 'center', gap: 6},
  threats: {
    paddingHorizontal: 10, paddingVertical: 5, justifyContent: 'center', minHeight: 44,
    borderWidth: 1, borderColor: outline, backgroundColor: glass, borderRadius: 7,
  },
  pause: {
    minWidth: 44, minHeight: 44, backgroundColor: glass, borderWidth: 1, borderColor: outline,
    borderRadius: 7, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 4,
  },
  pauseBar: {width: 3, height: 12, borderRadius: 1, backgroundColor: cream},
  bottom: {position: 'absolute', left: 0, right: 0, alignItems: 'center'},
  action: {
    minHeight: 44, minWidth: 104, borderWidth: 1, borderColor: outline, borderRadius: 7,
    justifyContent: 'center', alignItems: 'center', paddingHorizontal: 12, backgroundColor: glass,
  },
  actionText: {fontSize: 13, fontWeight: '800', color: cream},
  actionReady: {backgroundColor: amber, borderColor: amber},
  actionReadyText: {color: charcoal},
  actionDisabled: {opacity: 0.7},
  controlPressed: {opacity: 0.72},
  scrim: {
    ...StyleSheet.absoluteFill,
    backgroundColor: '#1016128A',
    justifyContent: 'center',
    alignItems: 'center',
  },
  menu: {
    width: '100%', maxWidth: 410, backgroundColor: '#18221EFA', borderWidth: 1,
    borderColor: outline, borderRadius: 11, paddingHorizontal: 26, paddingVertical: 20, gap: 8,
  },
  menuCompact: {paddingVertical: 13, gap: 6},
  title: {color: cream, fontSize: 38, lineHeight: 42, fontWeight: '900', letterSpacing: -1.2},
  titleCompact: {fontSize: 30, lineHeight: 34},
  description: {color: cream, fontSize: 14, lineHeight: 19},
  instructions: {color: '#C7B998', fontSize: 12, lineHeight: 17},
  menuActions: {flexDirection: 'row', gap: 9, marginTop: 7},
  primary: {
    alignItems: 'center', justifyContent: 'center', minHeight: 44, paddingHorizontal: 18,
    borderRadius: 6, backgroundColor: amber,
  },
  primaryText: {fontSize: 13, color: charcoal, fontWeight: '900'},
  secondary: {
    alignItems: 'center', justifyContent: 'center', minHeight: 44, paddingHorizontal: 14,
    borderRadius: 6, borderColor: outline, borderWidth: 1,
  },
  secondaryText: {fontSize: 13, color: cream, fontWeight: '700'},
});
