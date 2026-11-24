#ifndef ZOMBIEBOX_H
#define ZOMBIEBOX_H

#include <stdbool.h>
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

#ifdef __cplusplus
}
#endif
#endif
