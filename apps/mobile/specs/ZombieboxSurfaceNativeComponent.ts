import type {CodegenTypes, HostComponent, ViewProps} from 'react-native';
import {codegenNativeComponent} from 'react-native';

type Hud = Readonly<{
  health: CodegenTypes.Int32;
  supplies: CodegenTypes.Int32;
  armed: boolean;
}>;

export interface NativeProps extends ViewProps {
  readonly runId: string;
  readonly seed: string;
  readonly paused: boolean;
  readonly onHudChanged?: CodegenTypes.DirectEventHandler<Hud>;
}

export default codegenNativeComponent<NativeProps>(
  'ZombieboxSurface',
) as HostComponent<NativeProps>;
