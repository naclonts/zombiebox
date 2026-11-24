import type * as React from 'react';
import type {CodegenTypes, HostComponent, ViewProps} from 'react-native';
import {codegenNativeCommands, codegenNativeComponent} from 'react-native';

type Hud = Readonly<{
  health: CodegenTypes.Int32;
  supplies: CodegenTypes.Int32;
  zombies: CodegenTypes.Int32;
  armed: boolean;
  nearLoot: boolean;
  won: boolean;
  dead: boolean;
}>;

export interface NativeProps extends ViewProps {
  readonly runId: string;
  readonly seed: string;
  readonly paused: boolean;
  readonly onHudChanged?: CodegenTypes.DirectEventHandler<Hud>;
}

interface NativeCommands {
  requestUse: (viewRef: React.ComponentRef<HostComponent<NativeProps>>) => void;
}

export const Commands: NativeCommands = codegenNativeCommands<NativeCommands>({
  supportedCommands: ['requestUse'],
});

export default codegenNativeComponent<NativeProps>(
  'ZombieboxSurface',
) as HostComponent<NativeProps>;
