import React, {useEffect, useRef, useState} from 'react';
import {
  BackHandler,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
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
import {BUILD_STAMP, GIT_COMMIT} from './gitInfo';

const DEFAULT_MAX_HEALTH = 6;
const DEFAULT_ZOMBIES_TOTAL = 12;
const DEFAULT_SEED = '42';
const MAX_U64 = BigInt('18446744073709551615');

type Hud = Parameters<NonNullable<NativeProps['onHudChanged']>>[0]['nativeEvent'];
type Menu = 'start' | 'paused' | 'won' | 'dead';
type ScenarioId = 'county' | 'birch';
type ControlSchemeId = 'tap_stick' | 'outer_ring' | 'release_fire';
type CameraMode = 'isometric' | 'overhead' | 'orbit' | 'chase' | 'first_person';
const CAMERA_OPTIONS: ReadonlyArray<{id: CameraMode; label: string}> = [
  {id: 'isometric', label: 'ISO'}, {id: 'overhead', label: 'TOP'},
  {id: 'orbit', label: 'ORBIT'}, {id: 'chase', label: 'CHASE'},
  {id: 'first_person', label: 'FIRST'},
];

const VOLUME_STEPS = [0, 25, 50, 75, 100] as const;

const DISTRICT_NAMES: Readonly<Record<number, string>> = {
  0: 'BIRCH STREET',
  1: 'RESIDENTIAL',
  2: 'DOWNTOWN',
  3: 'HOSPITAL',
  4: 'POLICE PRECINCT',
  5: 'ARMY BASE',
  6: 'COUNTRYSIDE',
  7: 'WOODS',
};

function sanitizeSeed(raw: string): string | null {
  const s = raw.trim();
  if (!/^\d+$/.test(s)) return null;
  const value = BigInt(s);
  if (value > MAX_U64) return null;
  return value.toString();
}

function getInteractLabel(interactKind: number, floor: number): string | null {
  switch (interactKind) {
    case 1:
      return 'SEARCH';
    case 2:
      return 'OPEN DOOR';
    case 3:
      return 'CLOSE DOOR';
    case 4:
      return 'CLIMB';
    case 5:
      return `DROP ${Math.max(1, floor)}F`;
    case 6:
      return `STAIRS ${floor + 2}F`;
    case 7:
      return `STAIRS ${Math.max(1, floor)}F`;
    case 8:
      return 'MOUNT';
    case 9:
      return 'DISMOUNT';
    case 10:
      return 'RIGHT BIKE';
    default:
      return null;
  }
}

const SCENARIO_OPTIONS: ReadonlyArray<{id: ScenarioId; label: string}> = [
  {id: 'county', label: 'COUNTY'},
  {id: 'birch', label: 'BIRCH STREET'},
];

const SCHEME_OPTIONS: ReadonlyArray<{
  id: ControlSchemeId;
  label: string;
  hint: string;
}> = [
  {
    id: 'tap_stick',
    label: 'Tap Stick',
    hint: 'Drag right stick to aim, tap to fire, double-tap & hold to auto-fire.',
  },
  {
    id: 'outer_ring',
    label: 'Outer Ring',
    hint: 'Inner 78% aims cleanly; push past the amber ring to auto-fire.',
  },
  {
    id: 'release_fire',
    label: 'Release',
    hint: 'Drag to aim and release to fire; pull back to center to cancel safely.',
  },
];

const WEAPON_SLOTS: ReadonlyArray<{
  slot: number;
  short: string;
  title: string;
}> = [
  {slot: 0, short: 'AXE', title: 'AXE'},
  {slot: 1, short: 'PST', title: 'PISTOL'},
  {slot: 2, short: 'SHT', title: 'SHOTGUN'},
  {slot: 3, short: 'RKT', title: 'ROCKET'},
];

const menuContentByScenario: Record<
  ScenarioId,
  Record<Menu, {title: string; description: string | null; action: string}>
> = {
  county: {
    start: {
      title: 'Zombiebox',
      description:
        'Explore a seed-driven 512×512 county: scavenge residential neighborhoods, downtown, hospital, precinct, and army base.',
      action: 'Enter county',
    },
    paused: {
      title: 'Paused',
      description: null,
      action: 'Resume',
    },
    won: {
      title: 'County secured',
      description: 'Every infected threat in the county is down.',
      action: 'Play again',
    },
    dead: {
      title: 'Survivor lost',
      description: 'The infected overwhelmed you in the county.',
      action: 'Try again',
    },
  },
  birch: {
    start: {
      title: 'Zombiebox',
      description:
        'Scavenge the Birch Street houses for tools, firearms, and medkits, then clear the block.',
      action: 'Enter the block',
    },
    paused: {
      title: 'Paused',
      description: null,
      action: 'Resume',
    },
    won: {
      title: 'Block cleared',
      description: 'Birch Street is quiet. Every infected on the block is down.',
      action: 'Play again',
    },
    dead: {
      title: 'Survivor lost',
      description: 'The infected overwhelmed you on Birch Street.',
      action: 'Try again',
    },
  },
};

function Game() {
  const insets = useSafeAreaInsets();
  const {height, width} = useWindowDimensions();
  const surface = useRef<React.ComponentRef<typeof GameSurface>>(null);
  const pendingAtlas = useRef(false);

  const [run, setRun] = useState(1);
  const [scenario, setScenario] = useState<ScenarioId>('county');
  const [seed, setSeed] = useState<string>(DEFAULT_SEED);
  const [seedInput, setSeedInput] = useState<string>(DEFAULT_SEED);
  const [runStarted, setRunStarted] = useState<boolean>(false);
  const [menu, setMenu] = useState<Menu | null>('start');
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [hud, setHud] = useState<Hud | null>(null);

  const [controlScheme, setControlScheme] = useState<ControlSchemeId>('tap_stick');
  const [sfxVolumePct, setSfxVolumePct] = useState<number>(75);
  const [sfxMuted, setSfxMuted] = useState<boolean>(false);
  const [hapticsEnabled, setHapticsEnabled] = useState<boolean>(true);
  const [aimLookAhead, setAimLookAhead] = useState<boolean>(true);
  const [cameraMode, setCameraMode] = useState<CameraMode>('isometric');

  const activeMenu: Menu | null =
    menu ?? (hud?.won ? 'won' : hud?.dead ? 'dead' : null);
  const simPaused = activeMenu !== null || settingsOpen;
  const usableHeight = height - insets.top - insets.bottom;
  const usableWidth = width - insets.left - insets.right;
  const compact = usableHeight < 340;
  const narrow = usableWidth < 680;
  const menuWidth = Math.min(416, Math.max(284, Math.floor(width * 0.56)));

  const playClick = () => {
    if (surface.current) Commands.playUiClick(surface.current);
  };

  useEffect(() => {
    if (pendingAtlas.current && surface.current) {
      pendingAtlas.current = false;
      Commands.openCounty(surface.current);
    }
  }, [seed, run]);

  useEffect(() => {
    const back = BackHandler.addEventListener('hardwareBackPress', () => {
      if (settingsOpen) {
        playClick();
        setSettingsOpen(false);
        return true;
      }
      if (activeMenu === null || activeMenu === 'paused') {
        playClick();
        setMenu(activeMenu === null ? 'paused' : null);
        return true;
      }
      return false;
    });
    return () => back.remove();
  }, [activeMenu, settingsOpen]);

  const applySeedInput = (): string => {
    const clean = sanitizeSeed(seedInput);
    if (clean !== null) {
      setSeedInput(clean);
      if (clean !== seed) {
        setHud(null);
        setSeed(clean);
        setRun(previous => previous + 1);
      }
      return clean;
    }
    setSeedInput(seed);
    return seed;
  };

  const selectScenario = (next: ScenarioId) => {
    if (next === scenario) return;
    playClick();
    setHud(null);
    setScenario(next);
    setRun(previous => previous + 1);
  };

  const randomizeSeed = () => {
    playClick();
    const next = String(Math.floor(Math.random() * 0xffffffff) + 1);
    setSeedInput(next);
    if (next !== seed) {
      setHud(null);
      setSeed(next);
      setRun(previous => previous + 1);
    }
  };

  const openAtlas = () => {
    playClick();
    if (activeMenu === 'start') {
      const clean = sanitizeSeed(seedInput);
      if (clean !== null && clean !== seed) {
        pendingAtlas.current = true;
        setHud(null);
        setSeed(clean);
        setSeedInput(clean);
        setRun(previous => previous + 1);
        return;
      }
      if (clean === null) setSeedInput(seed);
    }
    if (surface.current) Commands.openCounty(surface.current);
  };

  const startRun = () => {
    playClick();
    if (activeMenu === 'start') {
      applySeedInput();
      setRunStarted(true);
    }
    setMenu(null);
  };

  const restart = () => {
    playClick();
    setHud(null);
    setRun(previous => previous + 1);
    setRunStarted(true);
    setSettingsOpen(false);
    setMenu(null);
  };

  const openSettings = () => {
    playClick();
    setSettingsOpen(true);
  };

  const closeSettings = () => {
    playClick();
    setSettingsOpen(false);
  };

  const handleInteract = () => {
    playClick();
    if (surface.current) Commands.requestInteract(surface.current);
  };

  const handleWeaponSlotPress = (slot: number, owned: boolean, active: boolean) => {
    if (!owned || !surface.current) return;
    playClick();
    if (active) {
      if (slot > 0 && hud?.canReload) {
        Commands.requestReload(surface.current);
      }
    } else {
      Commands.selectWeapon(surface.current, slot);
    }
  };

  const handleReload = () => {
    if (!surface.current) return;
    playClick();
    Commands.requestReload(surface.current);
  };

  const handleHeal = () => {
    if (!hud?.canHeal || !surface.current) return;
    playClick();
    Commands.requestHeal(surface.current);
  };

  const maxHealth = hud?.maxHealth ?? DEFAULT_MAX_HEALTH;
  const health = hud?.health ?? maxHealth;
  const lowHealth = health <= 2;
  const floor = hud?.floor ?? 0;
  const floorLabel = `${floor + 1}F`;
  const equippedWeapon = hud?.equippedWeapon ?? -1;
  const ownedMask = hud?.ownedWeaponsMask ?? 0;
  const zombies = hud?.zombies ?? (scenario === 'county' ? 0 : DEFAULT_ZOMBIES_TOTAL);
  const zombiesTotal =
    hud?.zombiesTotal ?? (scenario === 'county' ? 0 : DEFAULT_ZOMBIES_TOTAL);
  const cachesFound = hud?.cachesFound ?? 0;
  const cachesTotal = hud?.cachesTotal ?? 0;
  const interactKind = hud?.interactKind ?? 0;
  const interactLabel = getInteractLabel(interactKind, floor);
  const riding = hud?.controlKind === 1;
  const recovering = ((hud?.ridingFlags ?? 0) & 1) !== 0;
  const gripWarning = (hud?.gripUse ?? 0) >= 80;
  const ridingSpeed = Math.round((hud?.speedMphX10 ?? 0) / 10);
  const fps = hud?.fps ?? 0;
  const fpsLabel = fps > 0 ? `${fps} FPS` : '-- FPS';
  const medkits = hud?.medkits ?? 0;
  const canHeal = !!hud?.canHeal;
  const reloading = !!hud?.reloading;
  const canReload = !!hud?.canReload;
  const lowAmmo = !!hud?.lowAmmo;
  const dangerClose = !!hud?.dangerClose;
  const district = hud?.district ?? 0;
  const districtName = DISTRICT_NAMES[district] ?? 'COUNTY';
  const coordLabel = `${String(hud?.playerX ?? 0).padStart(3, '0')},${String(hud?.playerY ?? 0).padStart(3, '0')}`;

  const activeWeaponTitle =
    equippedWeapon >= 0 && equippedWeapon < WEAPON_SLOTS.length
      ? WEAPON_SLOTS[equippedWeapon].title
      : 'UNARMED';

  const activeWeaponReadout =
    equippedWeapon === -1
      ? 'Shove'
      : equippedWeapon === 0
        ? 'MELEE'
        : reloading
          ? `RELOAD ${hud?.reloadProgress ?? 0}%`
          : `${hud?.activeMag ?? 0}/${hud?.activeReserve ?? 0}`;

  const objectiveEyebrow = dangerClose
    ? 'DANGER CLOSE'
    : scenario === 'county'
      ? `${districtName} · ${coordLabel}`
      : 'OBJECTIVE';

  const objectiveText =
    scenario === 'county'
      ? hud?.objectiveKind === 3
        ? 'Survivor lost'
        : hud?.objectiveKind === 0
          ? 'Scavenge nearby buildings for gear'
          : 'Explore county & scavenge supplies'
      : hud?.objectiveKind === 2
        ? 'Block cleared'
        : hud?.objectiveKind === 3
          ? 'Survivor lost'
          : hud?.objectiveKind === 1
            ? `Clear Birch Street (${zombies} left)`
            : 'Scavenge houses for gear';

  const activeSchemeInfo =
    SCHEME_OPTIONS.find(item => item.id === controlScheme) ?? SCHEME_OPTIONS[0];

  const slotAmmoBadge = (slot: number): string => {
    if (slot === 0) return 'MELEE';
    if (!hud) return '0';
    if (slot === 1) return String(hud.pistolMag + hud.pistolReserve);
    if (slot === 2) return String(hud.shotgunMag + hud.shotgunReserve);
    return String(hud.rocketMag + hud.rocketReserve);
  };

  const content = activeMenu ? menuContentByScenario[scenario][activeMenu] : null;
  const showGameplayHud = !simPaused;
  const menuLocationLabel =
    scenario === 'county'
      ? activeMenu === 'start'
        ? `PROCEDURAL COUNTY · SEED ${seed}`
        : `${districtName} · SEED ${seed}`
      : 'BIRCH STREET · RESIDENTIAL BLOCK';

  return (
    <View style={styles.screen}>
      <GameSurface
        ref={surface}
        style={StyleSheet.absoluteFill}
        runId={String(run)}
        seed={seed}
        scenario={scenario}
        runStarted={runStarted}
        paused={simPaused}
        controlScheme={controlScheme}
        sfxVolumePct={sfxVolumePct}
        sfxMuted={sfxMuted}
        hapticsEnabled={hapticsEnabled}
        aimLookAhead={aimLookAhead}
        cameraMode={cameraMode}
        mountedIsometric={cameraMode === 'isometric'}
        onHudChanged={event => setHud(event.nativeEvent)}
        onAtlasTeleport={event => {
          const e = event.nativeEvent;
          setSeed(e.seed);
          setSeedInput(e.seed);
          setScenario('county');
          setRunStarted(true);
          setSettingsOpen(false);
          setMenu(null);
        }}
      />

      {showGameplayHud && (
        <>
          {/* Top HUD Bar */}
          <View
            pointerEvents="box-none"
            style={[
              styles.top,
              {
                top: insets.top + (compact ? 6 : 10),
                left: insets.left + 12,
                right: insets.right + 12,
              },
            ]}>
            <View pointerEvents="none" style={[styles.status, compact && styles.statusCompact]}>
              <View style={styles.floorBadge}>
                <Text style={styles.floorText}>{floorLabel}</Text>
              </View>

              <View
                accessible
                accessibilityLabel={`Health ${health} out of ${maxHealth}`}>
                <Text style={styles.statusLabel}>HEALTH</Text>
                <View style={styles.health}>
                  {Array.from({length: maxHealth}, (_, index) => (
                    <View
                      key={index}
                      style={[
                        styles.healthBar,
                        index < health &&
                          (lowHealth ? styles.healthLow : styles.healthFull),
                      ]}
                    />
                  ))}
                </View>
              </View>

              <View style={styles.weapon}>
                {riding ? (
                  <>
                    <Text style={styles.statusLabel}>BIKE {hud?.bikeCondition ?? 0}%</Text>
                    <Text style={[styles.statusValue, gripWarning && styles.statusValueWarn]}>
                      {ridingSpeed} MPH{gripWarning ? ' · LOW GRIP' : ''}
                    </Text>
                  </>
                ) : recovering ? (
                  <Text style={styles.statusLabel}>RECOVERING</Text>
                ) : (
                  <>
                    <Text style={styles.statusLabel}>{activeWeaponTitle}</Text>
                    <Text
                      style={[
                        styles.statusValue,
                        lowAmmo && !reloading && styles.statusValueWarn,
                        reloading && styles.statusValueAccent,
                      ]}>
                      {activeWeaponReadout}
                    </Text>
                  </>
                )}
              </View>
            </View>

            <View pointerEvents="none" style={styles.objective}>
              <View
                style={[
                  styles.objectiveCard,
                  dangerClose && styles.objectiveDanger,
                  compact && styles.objectiveCompact,
                ]}>
                <Text
                  style={[
                    styles.statusLabel,
                    dangerClose && styles.dangerLabel,
                  ]}>
                  {objectiveEyebrow}
                </Text>
                <Text
                  numberOfLines={1}
                  style={[
                    styles.objectiveText,
                    dangerClose && styles.dangerText,
                  ]}>
                  {dangerClose ? 'Rocket blast too close!' : objectiveText}
                </Text>
                {!dangerClose && cachesTotal > 0 && !compact && (
                  <Text style={styles.cacheSubtext}>
                    CACHES {cachesFound}/{cachesTotal}
                  </Text>
                )}
              </View>
            </View>

            <View pointerEvents="box-none" style={styles.right}>
              <View
                accessible
                accessibilityLabel={
                  scenario === 'county'
                    ? `${zombies} infected nearby`
                    : `${zombies} of ${zombiesTotal} infected remaining`
                }
                pointerEvents="none"
                style={[styles.threats, compact && styles.threatsCompact]}>
                <Text style={styles.statusLabel}>
                  {scenario === 'county' ? 'NEARBY' : 'INFECTED'}
                </Text>
                <Text style={styles.threatCount}>
                  {scenario === 'county' ? zombies : `${zombies}/${zombiesTotal}`}
                </Text>
              </View>

              <Text
                accessibilityLabel={`Frame rate ${fpsLabel}`}
                pointerEvents="none"
                style={styles.fps}>
                {fpsLabel}
              </Text>

              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Open county atlas"
                onPress={openAtlas}
                style={({pressed}) => [
                  styles.mapButton,
                  pressed && styles.controlPressed,
                ]}>
                <Text style={styles.mapButtonText}>MAP</Text>
              </Pressable>

              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Open settings"
                onPress={openSettings}
                style={({pressed}) => [
                  styles.iconButton,
                  pressed && styles.controlPressed,
                ]}>
                <View style={styles.sliderGlyph}>
                  <View style={styles.sliderRow}>
                    <View style={styles.sliderTrack} />
                    <View style={[styles.sliderDot, styles.sliderDotLeft]} />
                  </View>
                  <View style={styles.sliderRow}>
                    <View style={styles.sliderTrack} />
                    <View style={[styles.sliderDot, styles.sliderDotRight]} />
                  </View>
                  <View style={styles.sliderRow}>
                    <View style={styles.sliderTrack} />
                    <View style={[styles.sliderDot, styles.sliderDotMid]} />
                  </View>
                </View>
              </Pressable>

              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Pause game"
                onPress={() => {
                  playClick();
                  setMenu('paused');
                }}
                style={({pressed}) => [
                  styles.iconButton,
                  pressed && styles.controlPressed,
                ]}>
                <View style={styles.pauseRow}>
                  <View style={styles.pauseBar} />
                  <View style={styles.pauseBar} />
                </View>
              </Pressable>
            </View>
          </View>

          {/* Thumb-Adjacent Contextual Interact Button */}
          {!recovering && interactLabel !== null && (
            <View
              pointerEvents="box-none"
              style={[
                styles.interactAnchor,
                compact
                  ? {
                      right: insets.right + 144,
                      bottom: insets.bottom + 66,
                    }
                  : {
                      right: insets.right + 20,
                      bottom: insets.bottom + 148,
                    },
              ]}>
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={interactLabel}
                onPress={handleInteract}
                style={({pressed}) => [
                  styles.interactButton,
                  pressed && styles.controlPressed,
                ]}>
                <Text style={styles.interactText}>{interactLabel}</Text>
              </Pressable>
            </View>
          )}

          {/* Bottom-Center Hotbar + Quick-Heal + Contextual Reload Strip */}
          <View
            pointerEvents="box-none"
            style={[
              styles.bottom,
              (riding || recovering) && {display: 'none'},
              {bottom: insets.bottom + (compact ? 6 : 12)},
            ]}>
            <View style={styles.hotbarRow}>
              {WEAPON_SLOTS.map(item => {
                const owned = (ownedMask & (1 << item.slot)) !== 0;
                const active = equippedWeapon === item.slot;
                return (
                  <Pressable
                    key={item.slot}
                    accessibilityRole="button"
                    accessibilityLabel={`${item.title}${owned ? '' : ' locked'}`}
                    disabled={!owned}
                    onPress={() => handleWeaponSlotPress(item.slot, owned, active)}
                    style={({pressed}) => [
                      styles.hotbarSlot,
                      narrow && styles.hotbarSlotCompact,
                      !owned && styles.hotbarSlotLocked,
                      active && styles.hotbarSlotActive,
                      pressed && owned && styles.controlPressed,
                    ]}>
                    <Text
                      style={[
                        styles.slotName,
                        active && styles.slotNameActive,
                      ]}>
                      {item.short}
                    </Text>
                    <Text
                      style={[
                        styles.slotAmmo,
                        active && styles.slotAmmoActive,
                      ]}>
                      {owned ? slotAmmoBadge(item.slot) : '--'}
                    </Text>
                  </Pressable>
                );
              })}

              <Pressable
                accessibilityRole="button"
                accessibilityLabel={`Quick heal, ${medkits} medkits`}
                disabled={!canHeal}
                onPress={handleHeal}
                style={({pressed}) => [
                  styles.medkitPill,
                  narrow && styles.medkitPillCompact,
                  canHeal ? styles.medkitReady : styles.medkitDim,
                  pressed && canHeal && styles.controlPressed,
                ]}>
                <Text
                  style={[
                    styles.medkitTitle,
                    canHeal && styles.medkitTitleReady,
                  ]}>
                  + MED
                </Text>
                <Text
                  style={[
                    styles.medkitCount,
                    canHeal && styles.medkitTitleReady,
                  ]}>
                  x{medkits}
                </Text>
              </Pressable>

              {canReload && !reloading && (
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel="Reload weapon"
                  onPress={handleReload}
                  style={({pressed}) => [
                    styles.reloadPill,
                    (hud?.activeMag ?? 0) === 0 && styles.reloadPillUrgent,
                    pressed && styles.controlPressed,
                  ]}>
                  <Text
                    style={[
                      styles.reloadText,
                      (hud?.activeMag ?? 0) === 0 && styles.reloadTextUrgent,
                    ]}>
                    RLD
                  </Text>
                </Pressable>
              )}
            </View>
          </View>
        </>
      )}

      {/* Start / Pause / Won / Dead Menu Overlay */}
      {content && activeMenu && !settingsOpen && (
        <View
          accessibilityViewIsModal
          style={[
            styles.scrim,
            {
              paddingLeft: insets.left + 20,
              paddingRight: insets.right + 20,
              paddingTop: insets.top + 12,
              paddingBottom: insets.bottom + 12,
            },
          ]}>
          <View
            style={[
              styles.menu,
              {width: menuWidth},
              compact && styles.menuCompact,
            ]}>
            <Text style={styles.location}>{menuLocationLabel}</Text>
            <Text
              accessibilityRole="header"
              style={[
                styles.title,
                activeMenu === 'start' && styles.gameTitle,
                compact && styles.titleCompact,
              ]}>
              {content.title}
            </Text>
            {content.description && (
              <Text style={styles.description}>{content.description}</Text>
            )}
            {activeMenu === 'start' && (
              <View style={styles.startSetup}>
                <View style={styles.scenarioRow}>
                  {SCENARIO_OPTIONS.map(opt => {
                    const selected = scenario === opt.id;
                    return (
                      <Pressable
                        key={opt.id}
                        accessibilityRole="button"
                        onPress={() => selectScenario(opt.id)}
                        style={({pressed}) => [
                          styles.scenarioPill,
                          selected && styles.scenarioPillSelected,
                          pressed && styles.controlPressed,
                        ]}>
                        <Text
                          style={[
                            styles.scenarioPillText,
                            selected && styles.scenarioPillTextSelected,
                          ]}>
                          {opt.label}
                        </Text>
                      </Pressable>
                    );
                  })}
                </View>

                {scenario === 'county' && (
                  <View style={styles.seedRow}>
                    <Text style={styles.seedLabel}>SEED</Text>
                    <TextInput
                      accessibilityLabel="County seed"
                      keyboardType="number-pad"
                      returnKeyType="done"
                      value={seedInput}
                      onChangeText={setSeedInput}
                      onEndEditing={applySeedInput}
                      onSubmitEditing={applySeedInput}
                      placeholder="42"
                      placeholderTextColor={muted}
                      style={styles.seedInput}
                    />
                    <Pressable
                      accessibilityRole="button"
                      accessibilityLabel="Randomize seed"
                      onPress={randomizeSeed}
                      style={({pressed}) => [
                        styles.seedRndButton,
                        pressed && styles.controlPressed,
                      ]}>
                      <Text style={styles.seedRndText}>RND</Text>
                    </Pressable>
                  </View>
                )}
              </View>
            )}
            {activeMenu === 'start' && !compact && (
              <View style={styles.instructions}>
                <Text style={styles.instructionText}>
                  Left stick moves (light tilt walks quietly) · {activeSchemeInfo.label}:{' '}
                  {activeSchemeInfo.hint}
                </Text>
                <Text style={styles.instructionText}>
                  Tap prompts to search caches, open doors, vault windows, or use stairs.
                </Text>
              </View>
            )}
            <View style={styles.menuActions}>
              <Pressable
                accessibilityRole="button"
                onPress={
                  activeMenu === 'won' || activeMenu === 'dead'
                    ? restart
                    : startRun
                }
                style={({pressed}) => [
                  styles.primary,
                  pressed && styles.controlPressed,
                ]}>
                <Text style={styles.primaryText}>{content.action}</Text>
              </Pressable>

              {(activeMenu === 'start' || activeMenu === 'paused') && (
                <Pressable
                  accessibilityRole="button"
                  onPress={openAtlas}
                  style={({pressed}) => [
                    styles.secondary,
                    pressed && styles.controlPressed,
                  ]}>
                  <Text style={styles.secondaryText}>County atlas</Text>
                </Pressable>
              )}

              {(activeMenu === 'start' || activeMenu === 'paused') && (
                <Pressable
                  accessibilityRole="button"
                  onPress={openSettings}
                  style={({pressed}) => [
                    styles.secondary,
                    pressed && styles.controlPressed,
                  ]}>
                  <Text style={styles.secondaryText}>Settings</Text>
                </Pressable>
              )}

              {activeMenu === 'paused' && (
                <Pressable
                  accessibilityRole="button"
                  onPress={restart}
                  style={({pressed}) => [
                    styles.secondary,
                    pressed && styles.controlPressed,
                  ]}>
                  <Text style={styles.secondaryText}>Restart run</Text>
                </Pressable>
              )}
            </View>
          </View>
        </View>
      )}

      {/* Settings Modal */}
      {settingsOpen && (
        <View
          accessibilityViewIsModal
          style={[
            styles.scrim,
            styles.settingsScrim,
            {
              paddingLeft: insets.left + 16,
              paddingRight: insets.right + 16,
              paddingTop: insets.top + 10,
              paddingBottom: insets.bottom + 10,
            },
          ]}>
          <View
            style={[
              styles.settingsCard,
              compact && styles.settingsCardCompact,
            ]}>
            <View style={styles.settingsHeader}>
              <View>
                <Text style={styles.location}>SETTINGS</Text>
                <Text style={styles.settingsTitle}>Controls, Camera & Audio</Text>
              </View>
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Close settings"
                onPress={closeSettings}
                style={({pressed}) => [
                  styles.primary,
                  styles.doneButton,
                  pressed && styles.controlPressed,
                ]}>
                <Text style={styles.primaryText}>Done</Text>
              </Pressable>
            </View>

            <ScrollView style={styles.settingsScroll} contentContainerStyle={styles.settingsContent}>
            <View style={styles.settingGroup}>
              <Text style={styles.settingLabel}>CAMERA VIEW</Text>
              <View style={styles.segmentRow}>
                {CAMERA_OPTIONS.map(opt => (
                  <Pressable key={opt.id} accessibilityRole="button"
                    accessibilityLabel={`Camera ${opt.label}`}
                    onPress={() => { playClick(); setCameraMode(opt.id); }}
                    style={({pressed}) => [styles.segmentButton,
                      cameraMode === opt.id && styles.segmentButtonSelected,
                      pressed && styles.controlPressed]}>
                    <Text style={[styles.segmentText, cameraMode === opt.id && styles.segmentTextSelected]}>{opt.label}</Text>
                  </Pressable>
                ))}
              </View>
            </View>
            {/* Control Scheme */}
            <View style={styles.settingGroup}>
              <Text style={styles.settingLabel}>AIM & FIRE SCHEME</Text>
              <View style={styles.segmentRow}>
                {SCHEME_OPTIONS.map(opt => {
                  const selected = controlScheme === opt.id;
                  return (
                    <Pressable
                      key={opt.id}
                      accessibilityRole="button"
                      onPress={() => {
                        playClick();
                        setControlScheme(opt.id);
                      }}
                      style={({pressed}) => [
                        styles.segmentButton,
                        selected && styles.segmentButtonSelected,
                        pressed && styles.controlPressed,
                      ]}>
                      <Text
                        style={[
                          styles.segmentText,
                          selected && styles.segmentTextSelected,
                        ]}>
                        {opt.label}
                      </Text>
                    </Pressable>
                  );
                })}
              </View>
              <Text style={styles.settingHint}>{activeSchemeInfo.hint}</Text>
            </View>

            {/* SFX Volume + Mute */}
            <View style={styles.settingGroup}>
              <View style={styles.settingRowHeader}>
                <Text style={styles.settingLabel}>SFX VOLUME</Text>
                <Pressable
                  accessibilityRole="button"
                  onPress={() => {
                    playClick();
                    setSfxMuted(prev => !prev);
                  }}
                  style={({pressed}) => [
                    styles.muteChip,
                    sfxMuted && styles.muteChipActive,
                    pressed && styles.controlPressed,
                  ]}>
                  <Text
                    style={[
                      styles.muteChipText,
                      sfxMuted && styles.muteChipTextActive,
                    ]}>
                    {sfxMuted ? 'MUTED' : 'AUDIO ON'}
                  </Text>
                </Pressable>
              </View>
              <View style={styles.segmentRow}>
                {VOLUME_STEPS.map(step => {
                  const selected = !sfxMuted && sfxVolumePct === step;
                  return (
                    <Pressable
                      key={step}
                      accessibilityRole="button"
                      onPress={() => {
                        setSfxMuted(false);
                        setSfxVolumePct(step);
                        playClick();
                      }}
                      style={({pressed}) => [
                        styles.volumeStep,
                        selected && styles.segmentButtonSelected,
                        pressed && styles.controlPressed,
                      ]}>
                      <Text
                        style={[
                          styles.segmentText,
                          selected && styles.segmentTextSelected,
                        ]}>
                        {step}%
                      </Text>
                    </Pressable>
                  );
                })}
              </View>
            </View>

            {/* Haptics + Aim Look-Ahead Toggles */}
            <View style={styles.togglePair}>
              <Pressable
                accessibilityRole="button"
                onPress={() => {
                  playClick();
                  setHapticsEnabled(prev => !prev);
                }}
                style={({pressed}) => [
                  styles.toggleTile,
                  hapticsEnabled && styles.toggleTileOn,
                  pressed && styles.controlPressed,
                ]}>
                <Text style={styles.settingLabel}>HAPTICS</Text>
                <Text
                  style={[
                    styles.toggleValue,
                    hapticsEnabled && styles.toggleValueOn,
                  ]}>
                  {hapticsEnabled ? 'ON' : 'OFF'}
                </Text>
              </Pressable>

              <Pressable
                accessibilityRole="button"
                onPress={() => {
                  playClick();
                  setAimLookAhead(prev => !prev);
                }}
                style={({pressed}) => [
                  styles.toggleTile,
                  aimLookAhead && styles.toggleTileOn,
                  pressed && styles.controlPressed,
                ]}>
                <Text style={styles.settingLabel}>AIM LOOK-AHEAD</Text>
                <Text
                  style={[
                    styles.toggleValue,
                    aimLookAhead && styles.toggleValueOn,
                  ]}>
                  {aimLookAhead ? 'ON' : 'OFF'}
                </Text>
              </Pressable>
            </View>
            </ScrollView>
            <Text
              accessibilityLabel={`Build ${GIT_COMMIT}`}
              selectable
              style={styles.buildStamp}>
              {BUILD_STAMP}
            </Text>
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
const crimson = '#D94B43';
const emerald = '#4FC08D';

const styles = StyleSheet.create({
  screen: {flex: 1, backgroundColor: ink},
  top: {position: 'absolute', flexDirection: 'row', alignItems: 'flex-start', gap: 6},
  status: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingHorizontal: 8,
    minHeight: 44,
    backgroundColor: glass,
    borderWidth: 1,
    borderColor: outline,
  },
  statusCompact: {gap: 6, paddingHorizontal: 6, minHeight: 40},
  floorBadge: {
    paddingHorizontal: 6,
    paddingVertical: 4,
    backgroundColor: '#242D35',
    borderWidth: 1,
    borderColor: amber,
    alignItems: 'center',
    justifyContent: 'center',
  },
  floorText: {
    fontFamily: 'monospace',
    fontSize: 11,
    fontWeight: '900',
    color: amber,
  },
  statusLabel: {
    fontFamily: 'monospace',
    fontSize: 9,
    fontWeight: '700',
    letterSpacing: 0.5,
    color: muted,
  },
  health: {flexDirection: 'row', gap: 2, marginTop: 4},
  healthBar: {width: 8, height: 6, backgroundColor: '#464742'},
  healthFull: {backgroundColor: amber},
  healthLow: {backgroundColor: crimson},
  weapon: {borderLeftWidth: 1, borderColor: outline, paddingLeft: 8},
  statusValue: {fontSize: 12, color: bone, fontWeight: '800', fontFamily: 'monospace'},
  statusValueWarn: {color: crimson},
  statusValueAccent: {color: amber},
  objective: {flex: 1, alignItems: 'center'},
  objectiveCard: {
    alignItems: 'center',
    justifyContent: 'center',
    minHeight: 44,
    paddingHorizontal: 10,
    paddingVertical: 3,
    borderBottomWidth: 2,
    borderColor: amber,
    backgroundColor: glass,
    maxWidth: 260,
  },
  objectiveCompact: {minHeight: 40, paddingHorizontal: 8},
  objectiveDanger: {borderColor: crimson, backgroundColor: '#2D1416F2'},
  dangerLabel: {color: crimson},
  objectiveText: {color: bone, fontSize: 11, fontWeight: '700', textAlign: 'center'},
  dangerText: {color: '#FFD2CC'},
  cacheSubtext: {
    fontFamily: 'monospace',
    fontSize: 9,
    color: muted,
    marginTop: 1,
  },
  right: {flexDirection: 'row', alignItems: 'center', gap: 5},
  threats: {
    paddingHorizontal: 8,
    paddingVertical: 3,
    justifyContent: 'center',
    minHeight: 44,
    borderWidth: 1,
    borderColor: outline,
    backgroundColor: glass,
  },
  threatsCompact: {minHeight: 40, paddingHorizontal: 6},
  fps: {
    minWidth: 36,
    fontFamily: 'monospace',
    fontSize: 9,
    fontWeight: '700',
    letterSpacing: 0.5,
    color: muted,
    opacity: 0.85,
    textAlign: 'right',
  },
  threatCount: {
    color: bone,
    fontFamily: 'monospace',
    fontSize: 13,
    fontWeight: '800',
  },
  iconButton: {
    minWidth: 44,
    minHeight: 44,
    backgroundColor: glass,
    borderWidth: 1,
    borderColor: outline,
    alignItems: 'center',
    justifyContent: 'center',
  },
  mapButton: {
    minWidth: 44,
    minHeight: 44,
    paddingHorizontal: 8,
    backgroundColor: glass,
    borderWidth: 1,
    borderColor: amber,
    alignItems: 'center',
    justifyContent: 'center',
  },
  mapButtonText: {
    fontFamily: 'monospace',
    fontSize: 11,
    fontWeight: '900',
    letterSpacing: 0.6,
    color: amber,
  },
  sliderGlyph: {width: 18, height: 15, justifyContent: 'space-between'},
  sliderRow: {height: 4, justifyContent: 'center'},
  sliderTrack: {height: 1.5, backgroundColor: bone, width: '100%'},
  sliderDot: {
    position: 'absolute',
    width: 4,
    height: 4,
    backgroundColor: amber,
  },
  sliderDotLeft: {left: 3},
  sliderDotRight: {right: 3},
  sliderDotMid: {left: 7},
  pauseRow: {flexDirection: 'row', gap: 4},
  pauseBar: {width: 3, height: 12, backgroundColor: bone},
  interactAnchor: {position: 'absolute'},
  interactButton: {
    minHeight: 44,
    minWidth: 102,
    paddingHorizontal: 14,
    backgroundColor: amber,
    borderWidth: 1,
    borderColor: '#FFF0D4',
    alignItems: 'center',
    justifyContent: 'center',
  },
  interactText: {
    fontFamily: 'monospace',
    fontSize: 12,
    fontWeight: '900',
    letterSpacing: 0.6,
    color: ink,
  },
  bottom: {position: 'absolute', left: 0, right: 0, alignItems: 'center'},
  hotbarRow: {flexDirection: 'row', alignItems: 'center', gap: 4},
  hotbarSlot: {
    minWidth: 48,
    minHeight: 44,
    paddingHorizontal: 5,
    paddingVertical: 4,
    backgroundColor: glass,
    borderWidth: 1,
    borderColor: outline,
    alignItems: 'center',
    justifyContent: 'center',
  },
  hotbarSlotCompact: {minWidth: 42, paddingHorizontal: 3},
  hotbarSlotLocked: {opacity: 0.38},
  hotbarSlotActive: {
    borderColor: amber,
    backgroundColor: '#2A251CF5',
    borderWidth: 1.5,
  },
  slotName: {
    fontFamily: 'monospace',
    fontSize: 10,
    fontWeight: '800',
    color: muted,
  },
  slotNameActive: {color: amber},
  slotAmmo: {
    fontFamily: 'monospace',
    fontSize: 9,
    fontWeight: '700',
    color: bone,
    marginTop: 1,
  },
  slotAmmoActive: {color: '#FFF4DE'},
  medkitPill: {
    minWidth: 58,
    minHeight: 44,
    paddingHorizontal: 7,
    backgroundColor: glass,
    borderWidth: 1,
    borderColor: outline,
    alignItems: 'center',
    justifyContent: 'center',
    marginLeft: 2,
  },
  medkitPillCompact: {minWidth: 50, paddingHorizontal: 5},
  medkitReady: {borderColor: emerald, backgroundColor: '#152922F5'},
  medkitDim: {opacity: 0.55},
  medkitTitle: {
    fontFamily: 'monospace',
    fontSize: 10,
    fontWeight: '800',
    color: muted,
  },
  medkitCount: {
    fontFamily: 'monospace',
    fontSize: 10,
    fontWeight: '800',
    color: bone,
  },
  medkitTitleReady: {color: emerald},
  reloadPill: {
    minWidth: 44,
    minHeight: 44,
    paddingHorizontal: 6,
    backgroundColor: glass,
    borderWidth: 1,
    borderColor: amber,
    alignItems: 'center',
    justifyContent: 'center',
    marginLeft: 2,
  },
  reloadPillUrgent: {
    backgroundColor: amber,
    borderColor: '#FFF0D4',
  },
  reloadText: {
    fontFamily: 'monospace',
    fontSize: 10,
    fontWeight: '900',
    color: amber,
  },
  reloadTextUrgent: {color: ink},
  controlPressed: {opacity: 0.72},
  scrim: {
    ...StyleSheet.absoluteFill,
    backgroundColor: '#10131775',
    justifyContent: 'center',
    alignItems: 'flex-start',
  },
  settingsScrim: {alignItems: 'center'},
  menu: {
    maxWidth: '100%',
    backgroundColor: '#171C21FA',
    borderWidth: 1,
    borderColor: outline,
    borderLeftWidth: 3,
    borderLeftColor: amber,
    paddingHorizontal: 22,
    paddingVertical: 18,
    gap: 5,
  },
  menuCompact: {paddingHorizontal: 16, paddingVertical: 12, gap: 4},
  location: {
    color: amber,
    fontFamily: 'monospace',
    fontSize: 10,
    fontWeight: '700',
    letterSpacing: 1.8,
  },
  title: {
    color: bone,
    fontFamily: 'sans-serif-condensed',
    fontSize: 40,
    lineHeight: 44,
    fontWeight: '900',
    letterSpacing: -0.6,
  },
  gameTitle: {fontSize: 48, lineHeight: 50, textTransform: 'uppercase'},
  titleCompact: {fontSize: 32, lineHeight: 35},
  description: {color: bone, fontSize: 13, lineHeight: 18},
  startSetup: {gap: 6, marginTop: 4},
  scenarioRow: {flexDirection: 'row', gap: 6},
  scenarioPill: {
    flex: 1,
    minHeight: 34,
    borderWidth: 1,
    borderColor: outline,
    backgroundColor: '#1E252C',
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 8,
  },
  scenarioPillSelected: {
    borderColor: amber,
    backgroundColor: amber,
  },
  scenarioPillText: {
    fontFamily: 'monospace',
    fontSize: 11,
    fontWeight: '800',
    letterSpacing: 0.6,
    color: bone,
  },
  scenarioPillTextSelected: {color: ink},
  seedRow: {flexDirection: 'row', alignItems: 'center', gap: 6},
  seedLabel: {
    fontFamily: 'monospace',
    fontSize: 10,
    fontWeight: '800',
    letterSpacing: 0.8,
    color: muted,
  },
  seedInput: {
    flex: 1,
    minHeight: 34,
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderWidth: 1,
    borderColor: outline,
    backgroundColor: '#1E252C',
    color: bone,
    fontFamily: 'monospace',
    fontSize: 12,
    fontWeight: '700',
  },
  seedRndButton: {
    minWidth: 48,
    minHeight: 34,
    paddingHorizontal: 10,
    borderWidth: 1,
    borderColor: outline,
    backgroundColor: '#242D35',
    alignItems: 'center',
    justifyContent: 'center',
  },
  seedRndText: {
    fontFamily: 'monospace',
    fontSize: 11,
    fontWeight: '900',
    color: amber,
  },
  instructions: {
    borderTopWidth: 1,
    borderColor: outline,
    marginTop: 5,
    paddingTop: 6,
    gap: 3,
  },
  instructionText: {color: muted, fontSize: 11, lineHeight: 15},
  menuActions: {flexDirection: 'row', flexWrap: 'wrap', gap: 8, marginTop: 7},
  primary: {
    alignItems: 'center',
    justifyContent: 'center',
    minHeight: 44,
    paddingHorizontal: 16,
    backgroundColor: amber,
  },
  primaryText: {fontSize: 13, color: ink, fontWeight: '900'},
  secondary: {
    alignItems: 'center',
    justifyContent: 'center',
    minHeight: 44,
    paddingHorizontal: 14,
    borderColor: outline,
    borderWidth: 1,
  },
  secondaryText: {fontSize: 13, color: bone, fontWeight: '700'},
  settingsCard: {
    width: '100%',
    maxWidth: 480,
    maxHeight: '100%',
    backgroundColor: '#171C21FA',
    borderWidth: 1,
    borderColor: outline,
    borderLeftWidth: 3,
    borderLeftColor: amber,
    paddingHorizontal: 18,
    paddingVertical: 14,
    gap: 10,
  },
  settingsCardCompact: {paddingHorizontal: 14, paddingVertical: 10, gap: 7},
  settingsScroll: {flexShrink: 1},
  settingsContent: {gap: 10, paddingBottom: 4},
  settingsHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  settingsTitle: {
    color: bone,
    fontSize: 20,
    fontWeight: '900',
  },
  doneButton: {minHeight: 38, paddingHorizontal: 14},
  settingGroup: {gap: 5},
  settingRowHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  settingLabel: {
    fontFamily: 'monospace',
    fontSize: 10,
    fontWeight: '800',
    letterSpacing: 0.7,
    color: muted,
  },
  settingHint: {color: muted, fontSize: 11, lineHeight: 14},
  buildStamp: {
    fontFamily: 'monospace',
    fontSize: 9,
    fontWeight: '700',
    letterSpacing: 1.1,
    color: muted,
    opacity: 0.75,
    textAlign: 'right',
  },
  segmentRow: {flexDirection: 'row', gap: 5},
  segmentButton: {
    flex: 1,
    minHeight: 36,
    borderWidth: 1,
    borderColor: outline,
    backgroundColor: '#1E252C',
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 6,
  },
  volumeStep: {
    flex: 1,
    minHeight: 34,
    borderWidth: 1,
    borderColor: outline,
    backgroundColor: '#1E252C',
    alignItems: 'center',
    justifyContent: 'center',
  },
  segmentButtonSelected: {
    borderColor: amber,
    backgroundColor: amber,
  },
  segmentText: {
    fontFamily: 'monospace',
    fontSize: 11,
    fontWeight: '800',
    color: bone,
  },
  segmentTextSelected: {color: ink},
  muteChip: {
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderWidth: 1,
    borderColor: outline,
    backgroundColor: '#1E252C',
  },
  muteChipActive: {borderColor: crimson, backgroundColor: '#36191B'},
  muteChipText: {
    fontFamily: 'monospace',
    fontSize: 10,
    fontWeight: '800',
    color: bone,
  },
  muteChipTextActive: {color: '#FFB8B0'},
  togglePair: {flexDirection: 'row', gap: 8},
  toggleTile: {
    flex: 1,
    minHeight: 40,
    paddingHorizontal: 10,
    borderWidth: 1,
    borderColor: outline,
    backgroundColor: '#1E252C',
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  toggleTileOn: {borderColor: amber},
  toggleValue: {
    fontFamily: 'monospace',
    fontSize: 12,
    fontWeight: '900',
    color: muted,
  },
  toggleValueOn: {color: amber},
});
