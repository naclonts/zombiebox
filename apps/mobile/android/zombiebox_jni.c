#include <android/bitmap.h>
#include <jni.h>
#include <stdint.h>
#include <string.h>

#include "zombiebox.h"

static jint pack_u8x4(uint8_t b0, uint8_t b1, uint8_t b2, uint8_t b3) {
    uint32_t value = (uint32_t)b0
                   | ((uint32_t)b1 << 8)
                   | ((uint32_t)b2 << 16)
                   | ((uint32_t)b3 << 24);
    return (jint)value;
}

static jint pack_u16x2(uint16_t lo, uint16_t hi) {
    uint32_t value = (uint32_t)lo | ((uint32_t)hi << 16);
    return (jint)value;
}

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

JNIEXPORT jlong JNICALL Java_game_zombiebox_mobile_NativeGame_createV2(
    JNIEnv *env, jclass type, jlong seed) {
    (void)env;
    (void)type;
    return (jlong)(intptr_t)zb_action_v2_create((uint64_t)seed);
}

JNIEXPORT jlong JNICALL Java_game_zombiebox_mobile_NativeGame_createCountyV2(
    JNIEnv *env, jclass type, jlong seed) {
    (void)env;
    (void)type;
    return (jlong)(intptr_t)zb_action_v2_create_county((uint64_t)seed);
}

JNIEXPORT void JNICALL Java_game_zombiebox_mobile_NativeGame_destroyV2(
    JNIEnv *env, jclass type, jlong handle) {
    (void)env;
    (void)type;
    zb_action_v2_destroy((ZbAction *)(intptr_t)handle);
}

JNIEXPORT void JNICALL Java_game_zombiebox_mobile_NativeGame_resetClockV2(
    JNIEnv *env, jclass type, jlong handle) {
    (void)env;
    (void)type;
    zb_action_v2_reset_clock((ZbAction *)(intptr_t)handle);
}

JNIEXPORT jboolean JNICALL Java_game_zombiebox_mobile_NativeGame_frameV2(
    JNIEnv *env, jclass type, jlong handle, jobject bitmap, jlong frame_time_ns,
    jfloat move_x, jfloat move_y, jfloat aim_x, jfloat aim_y,
    jint action_flags, jint select_weapon, jintArray out_state) {
    (void)type;
    if (out_state == NULL || (*env)->GetArrayLength(env, out_state) < 12) {
        return JNI_FALSE;
    }

    AndroidBitmapInfo info;
    if (AndroidBitmap_getInfo(env, bitmap, &info) != ANDROID_BITMAP_RESULT_SUCCESS ||
        info.format != ANDROID_BITMAP_FORMAT_RGBA_8888) {
        return JNI_FALSE;
    }

    void *pixels;
    if (AndroidBitmap_lockPixels(env, bitmap, &pixels) != ANDROID_BITMAP_RESULT_SUCCESS) {
        return JNI_FALSE;
    }

    ZbActionFrameInput input;
    input.move_x = move_x;
    input.move_y = move_y;
    input.aim_x = aim_x;
    input.aim_y = aim_y;
    input.action_flags = (uint32_t)action_flags;
    input.select_weapon = (int32_t)select_weapon;

    ZbActionFrameOutput out;
    bool ok = zb_action_v2_frame((ZbAction *)(intptr_t)handle, pixels, info.width,
                                 info.height, info.stride, (uint64_t)frame_time_ns,
                                 &input, &out);
    AndroidBitmap_unlockPixels(env, bitmap);
    if (!ok) {
        return JNI_FALSE;
    }

    jint packed[12];
    packed[0] = pack_u8x4(out.health, out.max_health, out.floor, out.equipped_weapon);
    packed[1] = pack_u8x4(out.owned_weapons_mask, out.active_mag, out.active_mag_cap,
                          out.reload_progress);
    packed[2] = pack_u16x2(out.active_reserve, out.pistol_reserve);
    packed[3] = pack_u16x2(out.shotgun_reserve, out.rocket_reserve);
    packed[4] = pack_u8x4(out.pistol_mag, out.shotgun_mag, out.rocket_mag, out.medkits);
    packed[5] = pack_u16x2(out.zombies, out.zombies_total);
    packed[6] = pack_u8x4(out.caches_found, out.caches_total, out.interact_kind,
                          out.objective_kind);
    packed[7] = (jint)(uint32_t)out.status_flags;
    ZbActionLocation loc;
    memset(&loc, 0, sizeof loc);
    zb_action_v2_location((const ZbAction *)(intptr_t)handle, &loc);
    uint32_t ux = (uint32_t)(loc.x < 0 ? 0 : (loc.x > 4095 ? 4095 : loc.x));
    uint32_t uy = (uint32_t)(loc.y < 0 ? 0 : (loc.y > 4095 ? 4095 : loc.y));
    packed[8] = (jint)(ux
                       | (uy << 12)
                       | (((uint32_t)(loc.district & 0x0Fu)) << 24)
                       | (((uint32_t)(loc.mode & 0x0Fu)) << 28));
    packed[9] = (jint)(uint32_t)(out.audio_events & UINT64_C(0xFFFFFFFF));
    packed[10] = (jint)(uint32_t)(out.audio_events >> 32);
    packed[11] = 0;

    (*env)->SetIntArrayRegion(env, out_state, 0, 12, packed);
    return JNI_TRUE;
}

JNIEXPORT jboolean JNICALL Java_game_zombiebox_mobile_NativeGame_locationV2(
    JNIEnv *env, jclass type, jlong handle, jintArray out_loc) {
    (void)type;
    if (out_loc == NULL || (*env)->GetArrayLength(env, out_loc) < 5) {
        return JNI_FALSE;
    }
    ZbActionLocation loc;
    memset(&loc, 0, sizeof loc);
    if (!zb_action_v2_location((const ZbAction *)(intptr_t)handle, &loc)) {
        return JNI_FALSE;
    }
    jint buf[5];
    buf[0] = (jint)loc.x;
    buf[1] = (jint)loc.y;
    buf[2] = (jint)loc.floor;
    buf[3] = (jint)loc.mode;
    buf[4] = (jint)loc.district;
    (*env)->SetIntArrayRegion(env, out_loc, 0, 5, buf);
    return JNI_TRUE;
}

JNIEXPORT jboolean JNICALL Java_game_zombiebox_mobile_NativeGame_teleportV2(
    JNIEnv *env, jclass type, jlong handle, jint x, jint y, jint floor,
    jintArray out_resolved) {
    (void)type;
    ZbActionLocation resolved;
    memset(&resolved, 0, sizeof resolved);
    uint8_t clamped_floor = (uint8_t)(floor < 0 ? 0 : (floor > 255 ? 255 : floor));
    bool ok = zb_action_v2_teleport((ZbAction *)(intptr_t)handle, (int32_t)x, (int32_t)y,
                                    clamped_floor, &resolved);
    if (ok && out_resolved != NULL && (*env)->GetArrayLength(env, out_resolved) >= 4) {
        jint buf[4];
        buf[0] = (jint)resolved.x;
        buf[1] = (jint)resolved.y;
        buf[2] = (jint)resolved.floor;
        buf[3] = (jint)resolved.district;
        (*env)->SetIntArrayRegion(env, out_resolved, 0, 4, buf);
    }
    return ok ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jbyteArray JNICALL Java_game_zombiebox_mobile_NativeGame_countySurvey(
    JNIEnv *env, jclass type, jlong seed) {
    (void)type;
    char *json = zb_survey_v1_generate((uint64_t)seed);
    jsize length = (jsize)strlen(json);
    jbyteArray bytes = (*env)->NewByteArray(env, length);
    if (bytes != NULL) {
        (*env)->SetByteArrayRegion(env, bytes, 0, length, (const jbyte *)json);
    }
    zb_survey_v1_free(json);
    return bytes;
}
