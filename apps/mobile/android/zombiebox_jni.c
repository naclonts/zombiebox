#include <android/log.h>
#include <inttypes.h>
#include <android/native_window_jni.h>
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

static void write_frame_output(JNIEnv *env, jintArray out_state, const ZbAction *run, const ZbActionFrameOutputV3 *frame) {
    const ZbActionFrameOutput *out = &frame->base;
    jint packed[13];
    packed[0] = pack_u8x4(out->health, out->max_health, out->floor, out->equipped_weapon);
    packed[1] = pack_u8x4(out->owned_weapons_mask, out->active_mag, out->active_mag_cap,
                          out->reload_progress);
    packed[2] = pack_u16x2(out->active_reserve, out->pistol_reserve);
    packed[3] = pack_u16x2(out->shotgun_reserve, out->rocket_reserve);
    packed[4] = pack_u8x4(out->pistol_mag, out->shotgun_mag, out->rocket_mag, out->medkits);
    packed[5] = pack_u16x2(out->zombies, out->zombies_total);
    packed[6] = pack_u8x4(out->caches_found, out->caches_total, out->interact_kind,
                          out->objective_kind);
    packed[7] = (jint)(uint32_t)out->status_flags;
    ZbActionLocation loc;
    memset(&loc, 0, sizeof loc);
    zb_action_v2_location(run, &loc);
    uint32_t ux = (uint32_t)(loc.x < 0 ? 0 : (loc.x > 4095 ? 4095 : loc.x));
    uint32_t uy = (uint32_t)(loc.y < 0 ? 0 : (loc.y > 4095 ? 4095 : loc.y));
    packed[8] = (jint)(ux
                       | (uy << 12)
                       | (((uint32_t)(loc.district & 0x0Fu)) << 24)
                       | (((uint32_t)(loc.mode & 0x0Fu)) << 28));
    packed[9] = (jint)(uint32_t)(out->audio_events & UINT64_C(0xFFFFFFFF));
    packed[10] = (jint)(uint32_t)(out->audio_events >> 32);
    packed[11] = pack_u8x4(frame->control_kind, frame->grip_use, frame->engine_load,
                           frame->riding_flags);
    packed[12] = pack_u16x2(frame->speed_mph_x10, frame->condition_pct)
               | ((jint)frame->reserved << 24);

    (*env)->SetIntArrayRegion(env, out_state, 0, 13, packed);
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

JNIEXPORT jboolean JNICALL Java_game_zombiebox_mobile_NativeGame_attachSurface(
    JNIEnv *env, jclass type, jlong handle, jobject surface, jint width, jint height) {
    (void)type;
    if (!surface || width <= 0 || height <= 0) return JNI_FALSE;
    ANativeWindow *window = ANativeWindow_fromSurface(env, surface);
    if (!window) return JNI_FALSE;
    bool ok = zb_action_v2_attach_window((ZbAction *)(intptr_t)handle, window, (uint32_t)width, (uint32_t)height);
    ANativeWindow_release(window);
    return ok ? JNI_TRUE : JNI_FALSE;
}
JNIEXPORT void JNICALL Java_game_zombiebox_mobile_NativeGame_detachSurface(
    JNIEnv *env, jclass type, jlong handle) {
    (void)env; (void)type; zb_action_v2_detach_window((ZbAction *)(intptr_t)handle);
}
JNIEXPORT jboolean JNICALL Java_game_zombiebox_mobile_NativeGame_setCameraMode(
    JNIEnv *env, jclass type, jlong handle, jint mode) {
    (void)env;
    (void)type;
    if (mode < 0 || mode > 4) return JNI_FALSE;
    return zb_action_v2_camera_mode((ZbAction *)(intptr_t)handle, (uint8_t)mode)
        ? JNI_TRUE : JNI_FALSE;
}
JNIEXPORT jboolean JNICALL Java_game_zombiebox_mobile_NativeGame_resizeSurface(
    JNIEnv *env, jclass type, jlong handle, jint width, jint height) {
    (void)env; (void)type;
    return width > 0 && height > 0 && zb_action_v2_resize_window((ZbAction *)(intptr_t)handle, (uint32_t)width, (uint32_t)height) ? JNI_TRUE : JNI_FALSE;
}
JNIEXPORT jboolean JNICALL Java_game_zombiebox_mobile_NativeGame_frameSurface(
    JNIEnv *env, jclass type, jlong handle, jlong frame_time_ns, jfloat move_x, jfloat move_y,
    jfloat aim_x, jfloat aim_y, jint action_flags, jint select_weapon, jint control_kind,
    jfloat steer, jfloat throttle, jfloat brake, jfloat lean, jboolean mounted_isometric,
    jintArray out_state) {
    (void)type;
    if (!out_state || (*env)->GetArrayLength(env, out_state) < 13) return JNI_FALSE;
    ZbActionFrameInputV3 input = {
        .base = { move_x, move_y, aim_x, aim_y, (uint32_t)action_flags, (int32_t)select_weapon },
        .steer = steer, .throttle = throttle, .brake = brake, .lean = lean,
        .control_kind = (uint8_t)control_kind,
        .mounted_camera = mounted_isometric ? ZB_MOUNTED_ISOMETRIC : ZB_MOUNTED_CHASE,
    };
    ZbActionFrameOutputV3 out;
    ZbAction *run = (ZbAction *)(intptr_t)handle;
    if (!zb_action_v3_frame_surface(run, (uint64_t)frame_time_ns, &input, &out)) return JNI_FALSE;
    ZbFrameTiming timing;
    ZbFramePercentiles percentiles;
    if (zb_action_v2_frame_timing(run, &timing) && timing.frame_number % 120 == 0
            && zb_action_v2_frame_percentiles(run, &percentiles)) {
        __android_log_print(ANDROID_LOG_DEBUG, "Zombiebox3D",
            "frame=%" PRIu64 " samples=%" PRIu32 " p50_ms=%.2f p95_ms=%.2f p99_ms=%.2f "
            "prepare_ms=%.2f render_ms=%.2f chunks=%" PRIu32 " instances=%" PRIu32
            " triangles=%" PRIu64 " scale=%" PRIu32 " mph_x10=%u",
            timing.frame_number, percentiles.samples, percentiles.p50_ns / 1e6,
            percentiles.p95_ns / 1e6, percentiles.p99_ns / 1e6, timing.prepare_ns / 1e6,
            timing.render_ns / 1e6, timing.chunks_drawn, timing.instances_drawn,
            timing.triangles, timing.resolution_percent, (unsigned)out.speed_mph_x10);
    }
    write_frame_output(env, out_state, run, &out);
    return JNI_TRUE;
}

JNIEXPORT jstring JNICALL Java_game_zombiebox_mobile_NativeGame_readableTitle(
    JNIEnv *env, jclass type, jint id) {
    (void)type;
    const char *text = zb_action_readable_title((uint8_t)id);
    return text ? (*env)->NewStringUTF(env, text) : NULL;
}

JNIEXPORT jstring JNICALL Java_game_zombiebox_mobile_NativeGame_readableBody(
    JNIEnv *env, jclass type, jint id) {
    (void)type;
    const char *text = zb_action_readable_body((uint8_t)id);
    return text ? (*env)->NewStringUTF(env, text) : NULL;
}
