#ifndef ZOMBIEBOX_H
#define ZOMBIEBOX_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* A native host owns one handle per run. Serialize calls on its owner thread.
 * Destroy the handle once, after pending callbacks have finished. */
typedef struct ZbGame ZbGame;

enum ZbCommand {
    ZB_WAIT = 0,
    ZB_NORTH = 1,
    ZB_EAST = 2,
    ZB_SOUTH = 3,
    ZB_WEST = 4,
    ZB_SCAVENGE = 5,
    ZB_ATTACK = 6
};

typedef struct {
    uint64_t tick;
    int32_t player_x;
    int32_t player_y;
    uint16_t supplies;
    uint8_t health;
    uint8_t armed;
} ZbHud;

ZbGame *zb_v1_create(uint64_t seed);
uint32_t zb_v1_ticks_per_second(void);
/* Request one simulation tick. Returns false for a null handle or unknown
 * command. A run that has ended accepts commands and stays at its final tick. */
bool zb_v1_step(ZbGame *game, uint32_t command);
/* Copies a value into caller-owned storage. A null argument returns false. */
bool zb_v1_read_hud(const ZbGame *game, ZbHud *out);
void zb_v1_destroy(ZbGame *game);

/* Offline field atlas. The returned UTF-8 JSON belongs to the caller.
 * Release it exactly once with zb_survey_v1_free. */
char *zb_survey_v1_generate(uint64_t seed);
void zb_survey_v1_free(char *survey);

/* Action-game run and renderer. Call on one thread for the lifetime of the run. */
typedef struct ZbAction ZbAction;

/* Action v2 interface: multi-weapon hotbar, contextual interact, audio/haptics
 * event bitmask, look-ahead camera toggle, and full multi-level block HUD. */
enum ZbWeapon {
    ZB_WEAPON_UNCHANGED = -1,
    ZB_WEAPON_AXE = 0,
    ZB_WEAPON_PISTOL = 1,
    ZB_WEAPON_SHOTGUN = 2,
    ZB_WEAPON_ROCKET = 3
};

#define ZB_WEAPON_NONE UINT8_C(255)

enum ZbActionInputFlags {
    ZB_INPUT_FIRE_PULSE = 1u << 0,
    ZB_INPUT_FIRE_HOLD = 1u << 1,
    ZB_INPUT_INTERACT = 1u << 2,
    ZB_INPUT_RELOAD = 1u << 3,
    ZB_INPUT_HEAL = 1u << 4,
    ZB_INPUT_LOOK_AHEAD = 1u << 5
};

enum ZbInteractKind {
    ZB_INTERACT_NONE = 0,
    ZB_INTERACT_SEARCH = 1,
    ZB_INTERACT_OPEN_DOOR = 2,
    ZB_INTERACT_CLOSE_DOOR = 3,
    ZB_INTERACT_CLIMB_WINDOW = 4,
    ZB_INTERACT_DROP_WINDOW = 5,
    ZB_INTERACT_STAIRS_UP = 6,
    ZB_INTERACT_STAIRS_DOWN = 7,
    ZB_INTERACT_MOUNT = 8,
    ZB_INTERACT_DISMOUNT = 9,
    ZB_INTERACT_RIGHT_BIKE = 10
};

enum ZbObjectiveKind {
    ZB_OBJECTIVE_SCAVENGE = 0,
    ZB_OBJECTIVE_CLEAR_BLOCK = 1,
    ZB_OBJECTIVE_WON = 2,
    ZB_OBJECTIVE_DEAD = 3,
    ZB_OBJECTIVE_EXPLORE = 4
};

enum ZbActionMode {
    ZB_MODE_BIRCH_STREET = 0,
    ZB_MODE_COUNTY = 1
};

enum ZbDistrict {
    ZB_DISTRICT_BIRCH_STREET = 0,
    ZB_DISTRICT_RESIDENTIAL = 1,
    ZB_DISTRICT_DOWNTOWN = 2,
    ZB_DISTRICT_HOSPITAL = 3,
    ZB_DISTRICT_POLICE = 4,
    ZB_DISTRICT_ARMY_BASE = 5,
    ZB_DISTRICT_COUNTRYSIDE = 6,
    ZB_DISTRICT_WOODS = 7
};

typedef struct {
    int32_t x;
    int32_t y;
    uint8_t floor;
    uint8_t mode;
    uint8_t district;
    uint8_t reserved;
} ZbActionLocation;

enum ZbActionV2Status {
    ZB_V2_WON = 1u << 0,
    ZB_V2_DEAD = 1u << 1,
    ZB_V2_RELOADING = 1u << 2,
    ZB_V2_CAN_RELOAD = 1u << 3,
    ZB_V2_CAN_HEAL = 1u << 4,
    ZB_V2_LOW_AMMO = 1u << 5,
    ZB_V2_INDOORS = 1u << 6,
    ZB_V2_DANGER_CLOSE = 1u << 7
};

#define ZB_SFX_AXE_SWING         (UINT64_C(1) << 0)
#define ZB_SFX_AXE_HIT_FLESH     (UINT64_C(1) << 1)
#define ZB_SFX_AXE_HIT_WOOD      (UINT64_C(1) << 2)
#define ZB_SFX_PISTOL_FIRE       (UINT64_C(1) << 3)
#define ZB_SFX_SHOTGUN_FIRE      (UINT64_C(1) << 4)
#define ZB_SFX_ROCKET_LAUNCH     (UINT64_C(1) << 5)
#define ZB_SFX_EXPLOSION         (UINT64_C(1) << 6)
#define ZB_SFX_DRY_FIRE          (UINT64_C(1) << 7)
#define ZB_SFX_RELOAD_START      (UINT64_C(1) << 8)
#define ZB_SFX_RELOAD_FINISH     (UINT64_C(1) << 9)
#define ZB_SFX_GLASS_SHATTER     (UINT64_C(1) << 10)
#define ZB_SFX_WOOD_SPLINTER     (UINT64_C(1) << 11)
#define ZB_SFX_STONE_THUD        (UINT64_C(1) << 12)
#define ZB_SFX_STREETLIGHT_BREAK (UINT64_C(1) << 13)
#define ZB_SFX_DOOR_TOGGLE       (UINT64_C(1) << 14)
#define ZB_SFX_WINDOW_CLIMB      (UINT64_C(1) << 15)
#define ZB_SFX_STAIRS_STEP       (UINT64_C(1) << 16)
#define ZB_SFX_PICKUP            (UINT64_C(1) << 17)
#define ZB_SFX_MEDKIT_HEAL       (UINT64_C(1) << 18)
#define ZB_SFX_WEAPON_SWITCH     (UINT64_C(1) << 19)
#define ZB_SFX_MONSTER_ALERT     (UINT64_C(1) << 20)
#define ZB_SFX_MONSTER_HIT       (UINT64_C(1) << 21)
#define ZB_SFX_MONSTER_DOWN      (UINT64_C(1) << 22)
#define ZB_SFX_BLOATER_BURST     (UINT64_C(1) << 23)
#define ZB_SFX_SHOOTER_FIRE      (UINT64_C(1) << 24)
#define ZB_SFX_PLAYER_HURT       (UINT64_C(1) << 25)
#define ZB_SFX_WIN               (UINT64_C(1) << 26)
#define ZB_SFX_DEATH             (UINT64_C(1) << 27)
#define ZB_SFX_STRUCTURAL_BREACH (UINT64_C(1) << 28)
#define ZB_SFX_VEHICLE_CRASH     (UINT64_C(1) << 29)
#define ZB_SFX_VEHICLE_SKID      (UINT64_C(1) << 30)
#define ZB_SFX_VEHICLE_IMPACT    (UINT64_C(1) << 31)

typedef struct {
    float move_x;
    float move_y;
    float aim_x;
    float aim_y;
    uint32_t action_flags;
    int32_t select_weapon;
} ZbActionFrameInput;

typedef struct {
    uint64_t audio_events;
    uint16_t active_reserve;
    uint16_t pistol_reserve;
    uint16_t shotgun_reserve;
    uint16_t rocket_reserve;
    uint16_t zombies;
    uint16_t zombies_total;
    uint16_t status_flags;
    uint8_t health;
    uint8_t max_health;
    uint8_t floor;
    uint8_t equipped_weapon;
    uint8_t owned_weapons_mask;
    uint8_t active_mag;
    uint8_t active_mag_cap;
    uint8_t pistol_mag;
    uint8_t shotgun_mag;
    uint8_t rocket_mag;
    uint8_t reload_progress;
    uint8_t medkits;
    uint8_t caches_found;
    uint8_t caches_total;
    uint8_t interact_kind;
    uint8_t objective_kind;
} ZbActionFrameOutput;

enum ZbActionControlKind {
    ZB_CONTROL_ON_FOOT = 0,
    ZB_CONTROL_MOTORCYCLE = 1
};

enum ZbMountedCamera {
    ZB_MOUNTED_CHASE = 0,
    ZB_MOUNTED_ISOMETRIC = 1
};

enum ZbRidingFlags {
    ZB_RIDE_RECOVERING = 1u << 0
};

typedef struct {
    ZbActionFrameInput base;
    float steer, throttle, brake, lean;
    uint8_t control_kind;
    uint8_t mounted_camera;
    uint8_t reserved[2];
} ZbActionFrameInputV3;

typedef struct {
    ZbActionFrameOutput base;
    uint8_t control_kind;
    uint8_t grip_use;
    uint16_t speed_mph_x10;
    uint8_t engine_load;
    uint8_t riding_flags;
    uint8_t condition_pct;
    uint8_t reserved;
} ZbActionFrameOutputV3;

/* Create either the Birch Street encounter or the procedural county run. */
ZbAction *zb_action_v2_create(uint64_t seed);
ZbAction *zb_action_v2_create_county(uint64_t seed);
void zb_action_v2_destroy(ZbAction *run);
/* Suspend the run between display callbacks and discard all latched input. */
void zb_action_v2_reset_clock(ZbAction *run);
/* Step the 60 Hz simulation and return HUD and audio events. This entry point
 * accepts simulation-space directions. The surface entry point maps screen input. */
bool zb_action_v2_update(ZbAction *run, uint64_t frame_time_ns,
                         const ZbActionFrameInput *input, ZbActionFrameOutput *out);
/* Pass the previous frame's control kind. Motorcycle axes are raw stick values. */
bool zb_action_v3_update(ZbAction *run, uint64_t frame_time_ns,
                         const ZbActionFrameInputV3 *input, ZbActionFrameOutputV3 *out);

#ifdef __ANDROID__
typedef struct {
    uint64_t frame_number, frame_interval_ns, prepare_ns, render_ns, triangles;
    uint32_t chunks_drawn, chunks_culled, instances_drawn, resolution_percent;
} ZbFrameTiming;
typedef struct { uint64_t p50_ns, p95_ns, p99_ns; uint32_t samples; } ZbFramePercentiles;
/* The renderer retains its own ANativeWindow reference. Calls on a
 * run, including surface lifecycle and frames, must be serialized. */
bool zb_action_v2_attach_window(ZbAction *run, void *window, uint32_t width, uint32_t height);
void zb_action_v2_detach_window(ZbAction *run);
bool zb_action_v2_resize_window(ZbAction *run, uint32_t width, uint32_t height);
bool zb_action_v2_camera_mode(ZbAction *run, uint8_t mode);
bool zb_action_v2_frame_surface(ZbAction *run, uint64_t frame_time_ns,
                                const ZbActionFrameInput *input, ZbActionFrameOutput *out);
bool zb_action_v3_frame_surface(ZbAction *run, uint64_t frame_time_ns,
                                const ZbActionFrameInputV3 *input, ZbActionFrameOutputV3 *out);
bool zb_action_v2_frame_timing(const ZbAction *run, ZbFrameTiming *out);
bool zb_action_v2_frame_percentiles(const ZbAction *run, ZbFramePercentiles *out);
#endif
/* Read the survivor's current world tile, mode, and district code. */
bool zb_action_v2_location(const ZbAction *run, ZbActionLocation *out);
/* Move the survivor in an active County run to (x, y, floor) or a nearby
 * walkable tile. Returns false when the run or destination is unavailable.
 * Pass NULL for out_resolved when the resolved location is unneeded. */
bool zb_action_v2_teleport(ZbAction *run, int32_t x, int32_t y, uint8_t floor,
                           ZbActionLocation *out_resolved);

#ifdef __cplusplus
}
#endif
#endif
