package game.zombiebox.mobile;

import android.view.Surface;

final class NativeGame {
    static {
        System.loadLibrary("zombiebox_jni");
    }

    private NativeGame() {}

    static native byte[] countySurvey(long seed);

    static native long createV2(long seed);
    static native long createCountyV2(long seed);
    static native void destroyV2(long handle);
    static native void resetClockV2(long handle);
    static native boolean locationV2(long handle, int[] outLoc);
    static native boolean teleportV2(long handle, int x, int y, int floor, int[] outResolved);
    static native boolean attachSurface(long handle, Surface surface, int width, int height);
    static native void detachSurface(long handle);
    static native boolean setCameraMode(long handle, int mode);
    static native boolean resizeSurface(long handle, int width, int height);
    static native boolean frameSurface(long handle, long frameTimeNanos,
        float moveX, float moveY, float aimX, float aimY,
        int actionFlags, int selectWeapon, int controlKind,
        float steer, float throttle, float brake, float lean, boolean mountedIsometric, int[] outState);
    static native String readableTitle(int id);
    static native String readableBody(int id);
}
