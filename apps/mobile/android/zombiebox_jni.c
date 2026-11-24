#include <android/bitmap.h>
#include <jni.h>
#include <stdint.h>

#include "zombiebox.h"

JNIEXPORT jlong JNICALL Java_game_zombiebox_mobile_NativeGame_create(
    JNIEnv *env, jclass type, jlong seed) {
    (void)env;
    (void)type;
    return (jlong)(intptr_t)zb_action_v1_create((uint64_t)seed);
}

JNIEXPORT void JNICALL Java_game_zombiebox_mobile_NativeGame_destroy(
    JNIEnv *env, jclass type, jlong handle) {
    (void)env;
    (void)type;
    zb_action_v1_destroy((ZbAction *)(intptr_t)handle);
}

JNIEXPORT void JNICALL Java_game_zombiebox_mobile_NativeGame_resetClock(
    JNIEnv *env, jclass type, jlong handle) {
    (void)env;
    (void)type;
    zb_action_v1_reset_clock((ZbAction *)(intptr_t)handle);
}

JNIEXPORT jlong JNICALL Java_game_zombiebox_mobile_NativeGame_frame(
    JNIEnv *env, jclass type, jlong handle, jobject bitmap, jlong frame_time_ns,
    jfloat move_x, jfloat move_y, jfloat aim_x, jfloat aim_y, jboolean use_item) {
    (void)type;
    AndroidBitmapInfo info;
    if (AndroidBitmap_getInfo(env, bitmap, &info) != ANDROID_BITMAP_RESULT_SUCCESS ||
        info.format != ANDROID_BITMAP_FORMAT_RGBA_8888) {
        return -1;
    }

    void *pixels;
    if (AndroidBitmap_lockPixels(env, bitmap, &pixels) != ANDROID_BITMAP_RESULT_SUCCESS) {
        return -1;
    }
    jlong hud = zb_action_v1_frame((ZbAction *)(intptr_t)handle, pixels, info.width,
                                  info.height, info.stride, (uint64_t)frame_time_ns,
                                  move_x, move_y, aim_x, aim_y, use_item == JNI_TRUE);
    AndroidBitmap_unlockPixels(env, bitmap);
    return hud;
}
