import type * as React from 'react';
import type {CodegenTypes, HostComponent, ViewProps} from 'react-native';
import {codegenNativeCommands, codegenNativeComponent} from 'react-native';

type Hud = Readonly<{
  health: CodegenTypes.Int32;
  maxHealth: CodegenTypes.Int32;
  floor: CodegenTypes.Int32;
  equippedWeapon: CodegenTypes.Int32;
  ownedWeaponsMask: CodegenTypes.Int32;
  activeMag: CodegenTypes.Int32;
  activeMagCap: CodegenTypes.Int32;
  activeReserve: CodegenTypes.Int32;
  pistolMag: CodegenTypes.Int32;
  pistolReserve: CodegenTypes.Int32;
  shotgunMag: CodegenTypes.Int32;
  shotgunReserve: CodegenTypes.Int32;
  rocketMag: CodegenTypes.Int32;
  rocketReserve: CodegenTypes.Int32;
  reloadProgress: CodegenTypes.Int32;
  medkits: CodegenTypes.Int32;
  zombies: CodegenTypes.Int32;
  zombiesTotal: CodegenTypes.Int32;
  cachesFound: CodegenTypes.Int32;
  cachesTotal: CodegenTypes.Int32;
  interactKind: CodegenTypes.Int32;
  objectiveKind: CodegenTypes.Int32;
  won: boolean;
  dead: boolean;
  reloading: boolean;
  canReload: boolean;
  canHeal: boolean;
  lowAmmo: boolean;
  indoors: boolean;
  dangerClose: boolean;
  playerX: CodegenTypes.Int32;
  playerY: CodegenTypes.Int32;
  district: CodegenTypes.Int32;
  worldMode: CodegenTypes.Int32;
  controlKind: CodegenTypes.Int32;
  gripUse: CodegenTypes.Int32;
  engineLoad: CodegenTypes.Int32;
  speedMphX10: CodegenTypes.Int32;
  ridingFlags: CodegenTypes.Int32;
  bikeCondition: CodegenTypes.Int32;
  fps: CodegenTypes.Int32;
  readOpen: boolean;
  readId: CodegenTypes.Int32;
  readTitle: string;
  readBody: string;
}>;

type AtlasTeleportEvent = Readonly<{
  seed: string;
  x: CodegenTypes.Int32;
  y: CodegenTypes.Int32;
  floor: CodegenTypes.Int32;
  preservedRun: boolean;
  snapped: boolean;
}>;

export interface NativeProps extends ViewProps {
  readonly runId: string;
  readonly seed: string;
  readonly scenario?: string;
  readonly runStarted?: CodegenTypes.WithDefault<boolean, false>;
  readonly paused: boolean;
  readonly controlScheme?: string;
  readonly sfxVolumePct?: CodegenTypes.WithDefault<CodegenTypes.Int32, 75>;
  readonly sfxMuted?: CodegenTypes.WithDefault<boolean, false>;
  readonly hapticsEnabled?: CodegenTypes.WithDefault<boolean, true>;
  readonly aimLookAhead?: CodegenTypes.WithDefault<boolean, true>;
  readonly cameraMode?: string;
  readonly mountedIsometric?: CodegenTypes.WithDefault<boolean, false>;
  readonly onHudChanged?: CodegenTypes.DirectEventHandler<Hud>;
  readonly onAtlasTeleport?: CodegenTypes.DirectEventHandler<AtlasTeleportEvent>;
}

interface NativeCommands {
  openCounty: (viewRef: React.ComponentRef<HostComponent<NativeProps>>) => void;
  requestInteract: (
    viewRef: React.ComponentRef<HostComponent<NativeProps>>,
  ) => void;
  selectWeapon: (
    viewRef: React.ComponentRef<HostComponent<NativeProps>>,
    slot: CodegenTypes.Int32,
  ) => void;
  requestReload: (
    viewRef: React.ComponentRef<HostComponent<NativeProps>>,
  ) => void;
  requestHeal: (
    viewRef: React.ComponentRef<HostComponent<NativeProps>>,
  ) => void;
  playUiClick: (
    viewRef: React.ComponentRef<HostComponent<NativeProps>>,
  ) => void;
}

export const Commands: NativeCommands = codegenNativeCommands<NativeCommands>({
  supportedCommands: [
    'openCounty',
    'requestInteract',
    'selectWeapon',
    'requestReload',
    'requestHeal',
    'playUiClick',
  ],
});

export default codegenNativeComponent<NativeProps>(
  'ZombieboxSurface',
) as HostComponent<NativeProps>;
