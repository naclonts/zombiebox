#include "zombiebox.h"

#include <assert.h>
#include <stdio.h>

int main(void) {
    ZbGame *run = zb_v1_create(42);
    ZbHud spawn, current;
    assert(run && zb_v1_ticks_per_second() == 20);
    assert(zb_v1_read_hud(run, &spawn));
    assert(spawn.tick == 0 && spawn.health > 0);

    assert(!zb_v1_step(run, UINT32_MAX));
    assert(zb_v1_step(run, ZB_SCAVENGE));
    assert(zb_v1_read_hud(run, &current));
    assert(current.tick == 1 && current.supplies == 3 && current.armed);
    assert(zb_v1_step(run, ZB_EAST));
    assert(zb_v1_read_hud(run, &current));
    assert(current.tick == 2 && current.player_x == spawn.player_x + 1);
    assert(current.player_y == spawn.player_y);

    assert(!zb_v1_read_hud(run, NULL));
    assert(!zb_v1_step(NULL, ZB_WAIT));
    zb_v1_destroy(run);
    zb_v1_destroy(NULL);
    puts("C host created a Rust run, scavenged, moved east, read its HUD and released it.");
    return 0;
}
