package game.zombiebox.mobile;

import android.graphics.Bitmap;

final class NativeGame {
    static {
        System.loadLibrary("zombiebox_jni");
    }

    private NativeGame() {}

    static native long create(long seed);
    static native void destroy(long handle);
    static native void resetClock(long handle);
    static native long frame(long handle, Bitmap bitmap, long frameTimeNanos,
        float moveX, float moveY, float aimX, float aimY, boolean use);
}
