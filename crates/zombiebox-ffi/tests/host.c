#include "zombiebox.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

static void check_action_frame(void) {
    enum { WIDTH = 160, HEIGHT = 90, STRIDE = WIDTH * 4 + 8 };
    uint8_t pixels[STRIDE * HEIGHT];
    memset(pixels, 42, sizeof pixels);

    ZbAction *action = zb_action_v1_create(42);
    assert(action);
    assert(zb_action_v1_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 0,
                              0, 0, 0, 0, false) == ((3 << 16) | 5));
    assert(zb_action_v1_frame(action, pixels, WIDTH, HEIGHT, 1, 0,
                              0, 0, 0, 0, false) == -1);
    for (size_t y = 0; y < HEIGHT; ++y) {
        for (size_t x = 0; x < WIDTH; ++x) {
            assert(pixels[y * STRIDE + x * 4 + 3] == 255);
        }
        for (size_t x = WIDTH * 4; x < STRIDE; ++x) {
            assert(pixels[y * STRIDE + x] == 42);
        }
    }
    zb_action_v1_reset_clock(action);
    zb_action_v1_destroy(action);
}

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
    check_action_frame();
    puts("C host created a Rust run, scavenged, moved east, read its HUD and released it.");
    return 0;
}
