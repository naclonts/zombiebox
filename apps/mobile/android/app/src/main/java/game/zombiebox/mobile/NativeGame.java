package game.zombiebox.mobile;

import android.graphics.Bitmap;

final class NativeGame {
    static {
        System.loadLibrary("zombiebox_jni");
    }

    private NativeGame() {}

    static native byte[] countySurvey(long seed);

    static native long create(long seed);
    static native void destroy(long handle);
    static native void resetClock(long handle);
    static native long frame(long handle, Bitmap bitmap, long frameTimeNanos,
        float moveX, float moveY, float aimX, float aimY, boolean use);

    static native long createV2(long seed);
    static native long createCountyV2(long seed);
    static native void destroyV2(long handle);
    static native void resetClockV2(long handle);
    static native boolean frameV2(long handle, Bitmap bitmap, long frameTimeNanos,
        float moveX, float moveY, float aimX, float aimY,
        int actionFlags, int selectWeapon, int[] outState);
    static native boolean locationV2(long handle, int[] outLoc);
    static native boolean teleportV2(long handle, int x, int y, int floor, int[] outResolved);
}
