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

/* Action-game run and renderer. Call on one thread for the lifetime of the run. */
typedef struct ZbAction ZbAction;

enum ZbActionStatus {
    ZB_ACTION_ARMED = 1u << 24,
    ZB_ACTION_NEAR_LOOT = 1u << 25,
    ZB_ACTION_WON = 1u << 26,
    ZB_ACTION_DEAD = 1u << 27
};

ZbAction *zb_action_v1_create(uint64_t seed);
void zb_action_v1_destroy(ZbAction *run);
/* Suspend the run between display callbacks and discard pending touch input. */
void zb_action_v1_reset_clock(ZbAction *run);
/* Draw RGBA8888 into a host-owned buffer of height*stride bytes. The monotonic
 * timestamp is in nanoseconds. Stick vectors are screen-relative (+Y down).
 * use_item is a single press, retained until the next simulation tick.
 * Returns health, ammo and zombie count in the first three bytes of the result,
 * followed by ZbActionStatus bits, or -1 for invalid arguments. */
int64_t zb_action_v1_frame(ZbAction *run, uint8_t *pixels, uint32_t width,
                           uint32_t height, size_t stride, uint64_t frame_time_ns,
                           float move_x, float move_y, float aim_x, float aim_y,
                           bool use_item);

#ifdef __cplusplus
}
#endif
#endif
