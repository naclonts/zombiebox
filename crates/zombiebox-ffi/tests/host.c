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
                              0, 0, 0, 0, false) == ((12 << 16) | 6));
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

static void check_action_v2_frame(void) {
    enum { WIDTH = 160, HEIGHT = 90, STRIDE = WIDTH * 4 + 8 };
    uint8_t pixels[STRIDE * HEIGHT];
    memset(pixels, 77, sizeof pixels);

    ZbAction *action = zb_action_v2_create(42);
    assert(action);

    ZbActionFrameInput input;
    memset(&input, 0, sizeof input);
    input.select_weapon = ZB_WEAPON_UNCHANGED;
    input.action_flags = ZB_INPUT_LOOK_AHEAD;

    ZbActionFrameOutput out;
    memset(&out, 0, sizeof out);

    /* Frame 0 establishes the clock and populates initial Birch Street HUD. */
    assert(zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 1000000000ULL,
                              &input, &out));
    assert(out.health == 6);
    assert(out.max_health == 6);
    assert(out.floor == 0);
    assert(out.equipped_weapon == ZB_WEAPON_NONE);
    assert(out.owned_weapons_mask == 0);
    assert(out.zombies == 12);
    assert(out.zombies_total == 12);
    assert(out.caches_total > 0);
    assert(out.caches_found == 0);
    assert(out.objective_kind == ZB_OBJECTIVE_SCAVENGE);
    assert(out.audio_events == 0);

    for (size_t y = 0; y < HEIGHT; ++y) {
        for (size_t x = 0; x < WIDTH; ++x) {
            assert(pixels[y * STRIDE + x * 4 + 3] == 255);
        }
        for (size_t x = WIDTH * 4; x < STRIDE; ++x) {
            assert(pixels[y * STRIDE + x] == 77);
        }
    }

    /* Advance 20 ms with a fire pulse (unarmed shove) + look-ahead disabled. */
    input.action_flags = ZB_INPUT_FIRE_PULSE;
    assert(zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 1020000000ULL,
                              &input, &out));
    assert((out.audio_events & ZB_SFX_AXE_SWING) != 0);

    /* Walk slightly with interact and weapon selection (unowned weapon is a safe no-op). */
    input.move_x = 0.0f;
    input.move_y = -0.5f;
    input.aim_x = 1.0f;
    input.aim_y = 0.0f;
    input.action_flags = ZB_INPUT_INTERACT | ZB_INPUT_LOOK_AHEAD;
    input.select_weapon = ZB_WEAPON_AXE;
    assert(zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 1040000000ULL,
                              &input, &out));
    assert(out.health == 6);
    assert(out.equipped_weapon == ZB_WEAPON_NONE);

    /* Reset clock must discard any queued discrete input latched on a 0-tick frame. */
    zb_action_v2_reset_clock(action);
    memset(&input, 0, sizeof input);
    input.select_weapon = ZB_WEAPON_UNCHANGED;
    input.action_flags = ZB_INPUT_FIRE_PULSE;
    assert(zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 2000000000ULL,
                              &input, &out));
    assert(out.audio_events == 0);
    zb_action_v2_reset_clock(action);

    /* Re-establish clock with clean input, then advance 40 ms: no shove audio should fire. */
    input.action_flags = 0;
    assert(zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 3000000000ULL,
                              &input, &out));
    assert(zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 3040000000ULL,
                              &input, &out));
    assert((out.audio_events & ZB_SFX_AXE_SWING) == 0);

    /* Invalid argument handling. */
    assert(!zb_action_v2_frame(NULL, pixels, WIDTH, HEIGHT, STRIDE, 3060000000ULL,
                               &input, &out));
    assert(!zb_action_v2_frame(action, NULL, WIDTH, HEIGHT, STRIDE, 3060000000ULL,
                               &input, &out));
    assert(!zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 3060000000ULL,
                               NULL, &out));
    assert(!zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 3060000000ULL,
                               &input, NULL));
    assert(!zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, 1, 3060000000ULL,
                               &input, &out));
    input.select_weapon = 99;
    assert(!zb_action_v2_frame(action, pixels, WIDTH, HEIGHT, STRIDE, 3060000000ULL,
                               &input, &out));

    zb_action_v2_destroy(action);
    zb_action_v2_destroy(NULL);
}

static void check_action_v2_county_and_teleport(void) {
    enum { WIDTH = 160, HEIGHT = 90, STRIDE = WIDTH * 4 + 8 };
    uint8_t pixels[STRIDE * HEIGHT];
    memset(pixels, 55, sizeof pixels);

    ZbAction *county = zb_action_v2_create_county(42);
    assert(county);

    ZbActionLocation loc;
    memset(&loc, 0xA5, sizeof loc);
    assert(zb_action_v2_location(county, &loc));
    assert(loc.mode == ZB_MODE_COUNTY);
    assert(loc.district == ZB_DISTRICT_RESIDENTIAL);
    assert(loc.floor == 0);
    assert(loc.reserved == 0);
    assert(loc.x >= 0 && loc.x < 512);
    assert(loc.y >= 0 && loc.y < 512);

    ZbActionFrameInput input;
    memset(&input, 0, sizeof input);
    input.select_weapon = ZB_WEAPON_UNCHANGED;

    ZbActionFrameOutput out;
    memset(&out, 0, sizeof out);

    assert(zb_action_v2_frame(county, pixels, WIDTH, HEIGHT, STRIDE, 1000000000ULL,
                              &input, &out));
    assert(out.health == 6);
    assert(out.objective_kind == ZB_OBJECTIVE_SCAVENGE);

    ZbActionLocation resolved;
    memset(&resolved, 0xA5, sizeof resolved);
    assert(zb_action_v2_teleport(county, loc.x + 6, loc.y, 0, &resolved));
    assert(resolved.mode == ZB_MODE_COUNTY);
    assert(resolved.reserved == 0);
    assert(resolved.x >= 0 && resolved.x < 512);
    assert(resolved.y >= 0 && resolved.y < 512);

    assert(zb_action_v2_frame(county, pixels, WIDTH, HEIGHT, STRIDE, 1020000000ULL,
                              &input, &out));
    assert(out.health == 6);

    /* Teleporting with NULL out_resolved is valid; invalid handles return false. */
    assert(zb_action_v2_teleport(county, loc.x, loc.y, 0, NULL));
    assert(!zb_action_v2_location(NULL, &loc));
    assert(!zb_action_v2_location(county, NULL));
    assert(!zb_action_v2_teleport(NULL, 0, 0, 0, &resolved));

    /* Birch Street runs report Birch Street mode/district and reject teleportation. */
    ZbAction *birch = zb_action_v2_create(42);
    assert(birch);
    assert(zb_action_v2_location(birch, &loc));
    assert(loc.mode == ZB_MODE_BIRCH_STREET);
    assert(loc.district == ZB_DISTRICT_BIRCH_STREET);
    assert(!zb_action_v2_teleport(birch, loc.x + 1, loc.y, 0, &resolved));

    zb_action_v2_destroy(birch);
    zb_action_v2_destroy(county);
}

int main(void) {
    char *atlas = zb_survey_v1_generate(UINT64_MAX);
    assert(atlas && strstr(atlas, "\"seed\":\"18446744073709551615\""));
    assert(strstr(atlas, "\"landmarks\":[{"));
    zb_survey_v1_free(atlas);
    zb_survey_v1_free(NULL);

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
    check_action_v2_frame();
    check_action_v2_county_and_teleport();
    puts("C host created a Rust run, scavenged, moved east, read its HUD and exercised action v1 & v2.");
    return 0;
}
