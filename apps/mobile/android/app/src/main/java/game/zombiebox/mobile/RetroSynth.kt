package game.zombiebox.mobile

import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.sin

/**
 * Pure-Kotlin procedural 22,050 Hz 16-bit mono Genesis/PS1 retro sound bank.
 * Generates 30 compact PCM buffers (29 simulation SFX matching ZB_SFX_* bits 0..28
 * plus UI click at index 29) with deterministic noise and zero external dependencies.
 */
internal object RetroSynth {
    const val SAMPLE_RATE = 22050
    const val SFX_AXE_SWING = 0
    const val SFX_AXE_HIT_FLESH = 1
    const val SFX_AXE_HIT_WOOD = 2
    const val SFX_PISTOL_FIRE = 3
    const val SFX_SHOTGUN_FIRE = 4
    const val SFX_ROCKET_LAUNCH = 5
    const val SFX_EXPLOSION = 6
    const val SFX_DRY_FIRE = 7
    const val SFX_RELOAD_START = 8
    const val SFX_RELOAD_FINISH = 9
    const val SFX_GLASS_SHATTER = 10
    const val SFX_WOOD_SPLINTER = 11
    const val SFX_STONE_THUD = 12
    const val SFX_STREETLIGHT_BREAK = 13
    const val SFX_DOOR_TOGGLE = 14
    const val SFX_WINDOW_CLIMB = 15
    const val SFX_STAIRS_STEP = 16
    const val SFX_PICKUP = 17
    const val SFX_MEDKIT_HEAL = 18
    const val SFX_WEAPON_SWITCH = 19
    const val SFX_MONSTER_ALERT = 20
    const val SFX_MONSTER_HIT = 21
    const val SFX_MONSTER_DOWN = 22
    const val SFX_BLOATER_BURST = 23
    const val SFX_SHOOTER_FIRE = 24
    const val SFX_PLAYER_HURT = 25
    const val SFX_WIN = 26
    const val SFX_DEATH = 27
    const val SFX_STRUCTURAL_BREACH = 28
    const val SFX_UI_CLICK = 29
    const val SFX_COUNT = 30

    private const val TWO_PI = (2.0 * PI).toFloat()

    fun buildBank(): Array<ShortArray> = Array(SFX_COUNT) { id ->
        when (id) {
            SFX_AXE_SWING -> synthAxeSwing()
            SFX_AXE_HIT_FLESH -> synthAxeHitFlesh()
            SFX_AXE_HIT_WOOD -> synthAxeHitWood()
            SFX_PISTOL_FIRE -> synthPistolFire()
            SFX_SHOTGUN_FIRE -> synthShotgunFire()
            SFX_ROCKET_LAUNCH -> synthRocketLaunch()
            SFX_EXPLOSION -> synthExplosion()
            SFX_DRY_FIRE -> synthDryFire()
            SFX_RELOAD_START -> synthReloadStart()
            SFX_RELOAD_FINISH -> synthReloadFinish()
            SFX_GLASS_SHATTER -> synthGlassShatter()
            SFX_WOOD_SPLINTER -> synthWoodSplinter()
            SFX_STONE_THUD -> synthStoneThud()
            SFX_STREETLIGHT_BREAK -> synthStreetlightBreak()
            SFX_DOOR_TOGGLE -> synthDoorToggle()
            SFX_WINDOW_CLIMB -> synthWindowClimb()
            SFX_STAIRS_STEP -> synthStairsStep()
            SFX_PICKUP -> synthPickup()
            SFX_MEDKIT_HEAL -> synthMedkitHeal()
            SFX_WEAPON_SWITCH -> synthWeaponSwitch()
            SFX_MONSTER_ALERT -> synthMonsterAlert()
            SFX_MONSTER_HIT -> synthMonsterHit()
            SFX_MONSTER_DOWN -> synthMonsterDown()
            SFX_BLOATER_BURST -> synthBloaterBurst()
            SFX_SHOOTER_FIRE -> synthShooterFire()
            SFX_PLAYER_HURT -> synthPlayerHurt()
            SFX_WIN -> synthWin()
            SFX_DEATH -> synthDeath()
            SFX_STRUCTURAL_BREACH -> synthStructuralBreach()
            SFX_UI_CLICK -> synthUiClick()
            else -> ShortArray(1)
        }
    }

    private class XorShift32(seed: Int) {
        private var state = if (seed == 0) 0x13579BDF else seed
        fun nextFloat(): Float {
            var x = state
            x = x xor (x shl 13)
            x = x xor (x ushr 17)
            x = x xor (x shl 5)
            state = x
            return ((x and 0xFFFF) / 32767.5f) - 1f
        }
    }

    private class OnePoleLp(cutoffHz: Float) {
        private var y = 0f
        private var alpha = computeAlpha(cutoffHz)
        fun setCutoff(hz: Float) { alpha = computeAlpha(hz) }
        fun process(x: Float): Float {
            y += alpha * (x - y)
            return y
        }
        private fun computeAlpha(hz: Float): Float {
            val clamped = hz.coerceIn(20f, SAMPLE_RATE * 0.45f)
            val rc = 1f / (TWO_PI * clamped)
            val dt = 1f / SAMPLE_RATE
            return dt / (rc + dt)
        }
    }

    private class OnePoleHp(cutoffHz: Float) {
        private var prevX = 0f
        private var prevY = 0f
        private val alpha: Float
        init {
            val clamped = cutoffHz.coerceIn(20f, SAMPLE_RATE * 0.45f)
            val rc = 1f / (TWO_PI * clamped)
            val dt = 1f / SAMPLE_RATE
            alpha = rc / (rc + dt)
        }
        fun process(x: Float): Float {
            val y = alpha * (prevY + x - prevX)
            prevX = x
            prevY = y
            return y
        }
    }

    private class SvfBandpass(freqHz: Float, private val q: Float) {
        private var low = 0f
        private var band = 0f
        private var f = computeF(freqHz)
        fun setFreq(hz: Float) { f = computeF(hz) }
        fun process(x: Float): Float {
            val high = x - low - band / q
            band += f * high
            low += f * band
            return band
        }
        private fun computeF(hz: Float): Float {
            val clamped = hz.coerceIn(40f, SAMPLE_RATE * 0.30f)
            return (2f * sin(PI * clamped / SAMPLE_RATE).toFloat()).coerceAtMost(0.85f)
        }
    }

    private fun square(phase: Float): Float = if (sin(phase) >= 0f) 1f else -1f
    private fun saw(phase: Float): Float {
        val norm = (phase / TWO_PI) % 1f
        val p = if (norm < 0f) norm + 1f else norm
        return 2f * p - 1f
    }
    private fun tri(phase: Float): Float {
        val norm = (phase / TWO_PI) % 1f
        val p = if (norm < 0f) norm + 1f else norm
        return if (p < 0.5f) 4f * p - 1f else 3f - 4f * p
    }
    private fun fm2(carrierPhase: Float, modPhase: Float, index: Float): Float =
        sin(carrierPhase + index * sin(modPhase))

    private fun lerp(a: Float, b: Float, t: Float): Float = a + (b - a) * t
    private fun expSweep(fStart: Float, fEnd: Float, u: Float): Float =
        fStart * (fEnd / fStart).toDouble().pow(u.toDouble()).toFloat()

    private fun finalizePcm(samples: FloatArray, targetPeak: Float = 0.88f): ShortArray {
        val n = samples.size
        if (n == 0) return ShortArray(0)
        // Remove DC offset.
        var sum = 0.0
        for (v in samples) sum += v
        val mean = (sum / n).toFloat()
        var peak = 1e-6f
        for (i in 0 until n) {
            val v = samples[i] - mean
            samples[i] = v
            val a = abs(v)
            if (a > peak) peak = a
        }
        val gain = (targetPeak / peak) * 32767f
        val fadeIn = min(32, n / 8).coerceAtLeast(1)
        val fadeOut = min(64, n / 5).coerceAtLeast(1)
        return ShortArray(n) { i ->
            var env = 1f
            if (i < fadeIn) env *= i.toFloat() / fadeIn
            if (i >= n - fadeOut) env *= (n - 1 - i).toFloat() / fadeOut
            (samples[i] * gain * env).toInt().coerceIn(-32767, 32767).toShort()
        }
    }

    private fun makeBuffer(durationMs: Int, fill: (FloatArray) -> Unit): ShortArray {
        val n = max(64, (SAMPLE_RATE * durationMs) / 1000)
        val buf = FloatArray(n)
        fill(buf)
        return finalizePcm(buf)
    }

    // 0: Axe swing — brisk bandpassed air whoosh.
    private fun synthAxeSwing(): ShortArray = makeBuffer(135) { buf ->
        val rng = XorShift32(0x101)
        val bp = SvfBandpass(620f, 2.2f)
        val lp = OnePoleLp(1800f)
        val n = buf.size
        for (i in 0 until n) {
            val u = i.toFloat() / n
            val env = sin(u * PI).toFloat().pow(1.4f)
            bp.setFreq(expSweep(1150f, 320f, u))
            val air = bp.process(rng.nextFloat())
            buf[i] = lp.process(air) * env
        }
    }

    // 1: Axe hit flesh — low meaty impact + damp crunch.
    private fun synthAxeHitFlesh(): ShortArray = makeBuffer(145) { buf ->
        val rng = XorShift32(0x102)
        val lp = OnePoleLp(950f)
        val bp = SvfBandpass(460f, 1.4f)
        var ph = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val freq = expSweep(180f, 48f, u)
            ph += TWO_PI * freq / SAMPLE_RATE
            val thump = sin(ph) * exp(-t * 24f)
            val crunch = (lp.process(rng.nextFloat()) * 0.6f + bp.process(rng.nextFloat()) * 0.8f) * exp(-t * 28f)
            buf[i] = thump * 0.9f + crunch * 0.7f
        }
    }

    // 2: Axe hit wood — resonant woody FM knock + snappy crack.
    private fun synthAxeHitWood(): ShortArray = makeBuffer(130) { buf ->
        val rng = XorShift32(0x103)
        val bp = SvfBandpass(1150f, 2.8f)
        var cPh = 0f
        var mPh = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val f = expSweep(340f, 130f, u)
            cPh += TWO_PI * f / SAMPLE_RATE
            mPh += TWO_PI * (f * 2.41f) / SAMPLE_RATE
            val knock = fm2(cPh, mPh, 2.8f * exp(-t * 42f)) * exp(-t * 26f)
            val snap = bp.process(rng.nextFloat()) * exp(-t * 55f)
            buf[i] = knock * 0.85f + snap * 0.55f
        }
    }

    // 3: Pistol fire — sharp Genesis crack + tight punch body.
    private fun synthPistolFire(): ShortArray = makeBuffer(165) { buf ->
        val rng = XorShift32(0x104)
        val hp = OnePoleHp(480f)
        val lp = OnePoleLp(3600f)
        var ph = 0f
        var mPh = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val bodyFreq = expSweep(260f, 58f, u)
            ph += TWO_PI * bodyFreq / SAMPLE_RATE
            mPh += TWO_PI * (bodyFreq * 1.7f) / SAMPLE_RATE
            val crack = hp.process(rng.nextFloat()) * exp(-t * 46f)
            val tail = lp.process(rng.nextFloat()) * exp(-t * 18f)
            val body = fm2(ph, mPh, 1.8f * exp(-t * 30f)) * exp(-t * 26f)
            buf[i] = crack * 0.95f + body * 0.8f + tail * 0.35f
        }
    }

    // 4: Shotgun fire — heavy wide boom + roaring mid blast + pump tail.
    private fun synthShotgunFire(): ShortArray = makeBuffer(310) { buf ->
        val rng = XorShift32(0x105)
        val lp = OnePoleLp(2600f)
        val bp = SvfBandpass(780f, 1.1f)
        var ph = 0f
        var subPh = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val f = expSweep(155f, 36f, u)
            ph += TWO_PI * f / SAMPLE_RATE
            subPh += TWO_PI * (f * 0.55f) / SAMPLE_RATE
            val noise = rng.nextFloat()
            val blast = (lp.process(noise) * 0.85f + bp.process(noise) * 0.65f) * exp(-t * 11f)
            val punch = (sin(ph) + 0.5f * square(subPh)) * exp(-t * 16f)
            // Subtle mechanical pump click near end.
            val pump = if (t in 0.19f..0.24f) {
                val pt = t - 0.19f
                sin(TWO_PI * 920f * pt) * exp(-pt * 75f) * 0.28f
            } else 0f
            buf[i] = blast * 0.9f + punch * 0.75f + pump
        }
    }

    // 5: Rocket launch — pressurized thump + hissing propellant whoosh.
    private fun synthRocketLaunch(): ShortArray = makeBuffer(340) { buf ->
        val rng = XorShift32(0x106)
        val bp = SvfBandpass(950f, 1.8f)
        val hp = OnePoleHp(900f)
        var ph = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val thumpF = expSweep(140f, 45f, u)
            ph += TWO_PI * thumpF / SAMPLE_RATE
            val thump = sin(ph) * exp(-t * 20f)
            bp.setFreq(lerp(650f, 1900f, min(1f, u * 1.6f)))
            val whooshEnv = min(1f, t / 0.035f) * exp(-t * 5.2f)
            val noise = rng.nextFloat()
            val jet = (bp.process(noise) * 0.85f + hp.process(noise) * 0.45f) * whooshEnv
            buf[i] = thump * 0.75f + jet * 0.9f
        }
    }

    // 6: Explosion — deep sub drop + roaring rubble + sharp crackle.
    private fun synthExplosion(): ShortArray = makeBuffer(620) { buf ->
        val rng = XorShift32(0x107)
        val lp = OnePoleLp(950f)
        val bp = SvfBandpass(280f, 0.9f)
        var subPh = 0f
        var fmPh = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            lp.setCutoff(expSweep(2800f, 240f, u))
            val f = expSweep(115f, 26f, u)
            subPh += TWO_PI * f / SAMPLE_RATE
            fmPh += TWO_PI * (f * 1.42f) / SAMPLE_RATE
            val sub = fm2(subPh, fmPh, 2.2f * exp(-t * 7f)) * exp(-t * 5.5f)
            val noise = rng.nextFloat()
            val roar = (lp.process(noise) + bp.process(noise) * 0.6f) * exp(-t * 4.4f)
            val crack = if (t < 0.045f) noise * exp(-t * 60f) * 0.8f else 0f
            buf[i] = sub * 0.95f + roar * 0.85f + crack
        }
    }

    // 7: Dry fire — crisp metallic pin click.
    private fun synthDryFire(): ShortArray = makeBuffer(55) { buf ->
        val rng = XorShift32(0x108)
        val hp = OnePoleHp(1800f)
        var ph = 0f
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            ph += TWO_PI * 2450f / SAMPLE_RATE
            val ping = square(ph) * exp(-t * 110f)
            val click = hp.process(rng.nextFloat()) * exp(-t * 140f)
            buf[i] = ping * 0.55f + click * 0.85f
        }
    }

    // 8: Reload start — two quick mechanical clicks.
    private fun synthReloadStart(): ShortArray = makeBuffer(130) { buf ->
        val rng = XorShift32(0x109)
        val bp = SvfBandpass(1850f, 3.0f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            val c1 = exp(-t * 95f) * sin(TWO_PI * 1250f * t)
            val c2 = if (t >= 0.055f) {
                val dt = t - 0.055f
                exp(-dt * 85f) * sin(TWO_PI * 1650f * dt) * 0.9f
            } else 0f
            val slide = bp.process(rng.nextFloat()) * exp(-t * 24f) * 0.45f
            buf[i] = c1 * 0.7f + c2 + slide
        }
    }

    // 9: Reload finish — decisive two-stage bolt rack / slide lock.
    private fun synthReloadFinish(): ShortArray = makeBuffer(160) { buf ->
        val rng = XorShift32(0x10A)
        val bp = SvfBandpass(2100f, 3.2f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            val s1 = exp(-t * 65f) * (sin(TWO_PI * 980f * t) + 0.4f * square(TWO_PI * 1960f * t))
            val s2 = if (t >= 0.068f) {
                val dt = t - 0.068f
                exp(-dt * 70f) * (sin(TWO_PI * 1480f * dt) + 0.5f * sin(TWO_PI * 2920f * dt))
            } else 0f
            val grit = bp.process(rng.nextFloat()) * exp(-t * 28f) * 0.35f
            buf[i] = s1 * 0.6f + s2 * 0.85f + grit
        }
    }

    // 10: Glass shatter — inharmonic shimmering high pings + sparkling HP noise.
    private fun synthGlassShatter(): ShortArray = makeBuffer(300) { buf ->
        val rng = XorShift32(0x10B)
        val hp = OnePoleHp(2400f)
        val freqs = floatArrayOf(2620f, 3390f, 4180f, 5210f, 6190f)
        val delays = floatArrayOf(0f, 0.014f, 0.032f, 0.058f, 0.095f)
        val phases = FloatArray(freqs.size)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            var shards = 0f
            for (k in freqs.indices) {
                if (t >= delays[k]) {
                    val dt = t - delays[k]
                    phases[k] += TWO_PI * freqs[k] / SAMPLE_RATE
                    shards += sin(phases[k]) * exp(-dt * (22f + k * 5f))
                }
            }
            val sparkle = hp.process(rng.nextFloat()) * (exp(-t * 18f) + 0.4f * exp(-abs(t - 0.07f) * 45f))
            buf[i] = shards * 0.32f + sparkle * 0.85f
        }
    }

    // 11: Wood splinter — staggered dry cracks + resonant hollow snaps.
    private fun synthWoodSplinter(): ShortArray = makeBuffer(210) { buf ->
        val rng = XorShift32(0x10C)
        val bp = SvfBandpass(980f, 2.5f)
        val hits = floatArrayOf(0f, 0.032f, 0.074f, 0.118f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            var env = 0f
            var tone = 0f
            for (k in hits.indices) {
                if (t >= hits[k]) {
                    val dt = t - hits[k]
                    val e = exp(-dt * 52f) * (1f - k * 0.16f)
                    env += e
                    tone += sin(TWO_PI * (320f + k * 95f) * dt) * e
                }
            }
            val crack = bp.process(rng.nextFloat()) * env
            buf[i] = crack * 0.85f + tone * 0.45f
        }
    }

    // 12: Stone thud — dense low-mid masonry hit + dry dust puff.
    private fun synthStoneThud(): ShortArray = makeBuffer(120) { buf ->
        val rng = XorShift32(0x10D)
        val lp = OnePoleLp(720f)
        var ph = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val f = expSweep(190f, 68f, u)
            ph += TWO_PI * f / SAMPLE_RATE
            val thud = tri(ph) * exp(-t * 32f)
            val grit = lp.process(rng.nextFloat()) * exp(-t * 36f)
            buf[i] = thud * 0.85f + grit * 0.65f
        }
    }

    // 13: Streetlight break — metallic hollow pole clang + arc buzz + glass pop.
    private fun synthStreetlightBreak(): ShortArray = makeBuffer(360) { buf ->
        val rng = XorShift32(0x10E)
        val hp = OnePoleHp(2200f)
        var cPh = 0f
        var mPh = 0f
        var buzzPh = 0f
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            cPh += TWO_PI * 640f / SAMPLE_RATE
            mPh += TWO_PI * 1742f / SAMPLE_RATE
            buzzPh += TWO_PI * 120f / SAMPLE_RATE
            val clang = fm2(cPh, mPh, 3.8f * exp(-t * 8f)) * exp(-t * 7.5f)
            val pop = hp.process(rng.nextFloat()) * exp(-t * 38f)
            val buzz = if (t in 0.04f..0.24f) {
                val bt = t - 0.04f
                square(buzzPh) * (0.4f + 0.6f * abs(rng.nextFloat())) * exp(-bt * 11f) * 0.45f
            } else 0f
            buf[i] = clang * 0.75f + pop * 0.7f + buzz
        }
    }

    // 14: Door toggle — latch click + hollow wooden swing thud.
    private fun synthDoorToggle(): ShortArray = makeBuffer(150) { buf ->
        val rng = XorShift32(0x10F)
        val lp = OnePoleLp(900f)
        var ph = 0f
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            val latch = sin(TWO_PI * 1420f * t) * exp(-t * 95f) * 0.6f
            val body = if (t >= 0.025f) {
                val dt = t - 0.025f
                ph += TWO_PI * lerp(180f, 115f, min(1f, dt * 10f)) / SAMPLE_RATE
                (sin(ph) * 0.75f + lp.process(rng.nextFloat()) * 0.4f) * exp(-dt * 26f)
            } else 0f
            buf[i] = latch + body
        }
    }

    // 15: Window climb — cloth brush + sill thump.
    private fun synthWindowClimb(): ShortArray = makeBuffer(140) { buf ->
        val rng = XorShift32(0x110)
        val bp = SvfBandpass(720f, 1.5f)
        var ph = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val rustle = bp.process(rng.nextFloat()) * sin(u * PI).toFloat() * 0.65f
            val step = if (t >= 0.055f) {
                val dt = t - 0.055f
                ph += TWO_PI * 110f / SAMPLE_RATE
                sin(ph) * exp(-dt * 34f) * 0.8f
            } else 0f
            buf[i] = rustle + step
        }
    }

    // 16: Stairs step — three brisk wooden stair footfalls.
    private fun synthStairsStep(): ShortArray = makeBuffer(175) { buf ->
        val rng = XorShift32(0x111)
        val lp = OnePoleLp(650f)
        val taps = floatArrayOf(0f, 0.055f, 0.112f)
        val pitches = floatArrayOf(155f, 185f, 220f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            var out = 0f
            for (k in taps.indices) {
                if (t >= taps[k]) {
                    val dt = t - taps[k]
                    val e = exp(-dt * 52f)
                    out += (sin(TWO_PI * pitches[k] * dt) * 0.7f + lp.process(rng.nextFloat()) * 0.45f) * e
                }
            }
            buf[i] = out
        }
    }

    // 17: Pickup — bright 3-note Genesis FM chime (C5-E5-G5).
    private fun synthPickup(): ShortArray = makeBuffer(240) { buf ->
        val notes = floatArrayOf(523.25f, 659.25f, 783.99f)
        val starts = floatArrayOf(0f, 0.06f, 0.12f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            var out = 0f
            for (k in notes.indices) {
                if (t >= starts[k]) {
                    val dt = t - starts[k]
                    val c = TWO_PI * notes[k] * dt
                    val m = TWO_PI * notes[k] * 2f * dt
                    out += fm2(c, m, 1.4f * exp(-dt * 14f)) * exp(-dt * 12f)
                }
            }
            buf[i] = out * 0.45f
        }
    }

    // 18: Medkit heal — soothing warm ascending harmonic swell.
    private fun synthMedkitHeal(): ShortArray = makeBuffer(320) { buf ->
        val notes = floatArrayOf(440f, 554.37f, 659.25f, 880f)
        val starts = floatArrayOf(0f, 0.06f, 0.12f, 0.17f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            var out = 0f
            for (k in notes.indices) {
                if (t >= starts[k]) {
                    val dt = t - starts[k]
                    val attack = min(1f, dt / 0.015f)
                    val c = TWO_PI * notes[k] * dt
                    val m = TWO_PI * notes[k] * 1.5f * dt
                    out += fm2(c, m, 0.85f * exp(-dt * 8f)) * attack * exp(-dt * 9.5f)
                }
            }
            buf[i] = out * 0.4f
        }
    }

    // 19: Weapon switch — crisp tactical holster/metal snap.
    private fun synthWeaponSwitch(): ShortArray = makeBuffer(80) { buf ->
        val rng = XorShift32(0x112)
        val bp = SvfBandpass(1600f, 2.6f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            val snap = bp.process(rng.nextFloat()) * exp(-t * 58f)
            val click = sin(TWO_PI * 1380f * t) * exp(-t * 85f)
            val tick2 = if (t >= 0.028f) {
                val dt = t - 0.028f
                sin(TWO_PI * 1920f * dt) * exp(-dt * 90f) * 0.7f
            } else 0f
            buf[i] = snap * 0.5f + click * 0.65f + tick2
        }
    }

    // 20: Monster alert — raspy guttural FM growl slide.
    private fun synthMonsterAlert(): ShortArray = makeBuffer(260) { buf ->
        val rng = XorShift32(0x113)
        val bp = SvfBandpass(420f, 2.0f)
        var cPh = 0f
        var mPh = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val f = lerp(115f, 175f, sin(u * PI).toFloat())
            cPh += TWO_PI * f / SAMPLE_RATE
            mPh += TWO_PI * (f * 0.48f) / SAMPLE_RATE
            val env = min(1f, t / 0.03f) * exp(-t * 6.5f)
            val growl = fm2(cPh, mPh, 3.2f) * 0.7f + saw(cPh * 0.5f) * 0.35f
            val rasp = bp.process(rng.nextFloat()) * 0.45f
            buf[i] = (growl + rasp) * env
        }
    }

    // 21: Monster hit — punchy wet mid-low impact.
    private fun synthMonsterHit(): ShortArray = makeBuffer(115) { buf ->
        val rng = XorShift32(0x114)
        val lp = OnePoleLp(1100f)
        var ph = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            ph += TWO_PI * expSweep(210f, 65f, u) / SAMPLE_RATE
            val thump = sin(ph) * exp(-t * 30f)
            val wet = lp.process(rng.nextFloat()) * exp(-t * 34f)
            buf[i] = thump * 0.85f + wet * 0.6f
        }
    }

    // 22: Monster down — descending slump groan + floor thud.
    private fun synthMonsterDown(): ShortArray = makeBuffer(240) { buf ->
        val rng = XorShift32(0x115)
        val lp = OnePoleLp(520f)
        var cPh = 0f
        var mPh = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val f = expSweep(165f, 52f, u)
            cPh += TWO_PI * f / SAMPLE_RATE
            mPh += TWO_PI * (f * 0.53f) / SAMPLE_RATE
            val groan = fm2(cPh, mPh, 2.2f) * exp(-t * 9.5f)
            val slump = if (t >= 0.09f) {
                val dt = t - 0.09f
                (sin(TWO_PI * 72f * dt) * 0.7f + lp.process(rng.nextFloat()) * 0.6f) * exp(-dt * 22f)
            } else 0f
            buf[i] = groan * 0.65f + slump * 0.85f
        }
    }

    // 23: Bloater burst — pressurized wet rupture + caustic hiss.
    private fun synthBloaterBurst(): ShortArray = makeBuffer(360) { buf ->
        val rng = XorShift32(0x116)
        val lp = OnePoleLp(1400f)
        val hp = OnePoleHp(1600f)
        var ph = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            ph += TWO_PI * expSweep(135f, 32f, u) / SAMPLE_RATE
            val pop = sin(ph) * exp(-t * 14f)
            val noise = rng.nextFloat()
            val splat = lp.process(noise) * exp(-t * 10f)
            val sizzle = hp.process(noise) * min(1f, t / 0.03f) * exp(-t * 6.5f)
            buf[i] = pop * 0.9f + splat * 0.7f + sizzle * 0.55f
        }
    }

    // 24: Shooter fire — sharp venomous descending FM zip.
    private fun synthShooterFire(): ShortArray = makeBuffer(175) { buf ->
        val rng = XorShift32(0x117)
        val hp = OnePoleHp(1200f)
        var cPh = 0f
        var mPh = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val f = expSweep(1450f, 340f, u)
            cPh += TWO_PI * f / SAMPLE_RATE
            mPh += TWO_PI * (f * 2.1f) / SAMPLE_RATE
            val zip = fm2(cPh, mPh, 2.4f) * exp(-t * 15f)
            val spit = hp.process(rng.nextFloat()) * exp(-t * 36f)
            buf[i] = zip * 0.8f + spit * 0.5f
        }
    }

    // 25: Player hurt — heavy dull impact + distorted warning sting.
    private fun synthPlayerHurt(): ShortArray = makeBuffer(220) { buf ->
        val rng = XorShift32(0x118)
        val lp = OnePoleLp(850f)
        var ph = 0f
        var mPh = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            val f = expSweep(230f, 62f, u)
            ph += TWO_PI * f / SAMPLE_RATE
            mPh += TWO_PI * (f * 1.37f) / SAMPLE_RATE
            val sting = (fm2(ph, mPh, 3.0f) + 0.45f * square(ph * 0.5f)) * exp(-t * 11f)
            val thud = lp.process(rng.nextFloat()) * exp(-t * 22f)
            buf[i] = sting * 0.75f + thud * 0.65f
        }
    }

    // 26: Win — heroic 4-note Genesis FM fanfare (C5-E5-G5-C6).
    private fun synthWin(): ShortArray = makeBuffer(560) { buf ->
        val notes = floatArrayOf(523.25f, 659.25f, 783.99f, 1046.50f)
        val starts = floatArrayOf(0f, 0.09f, 0.18f, 0.27f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            var out = 0f
            for (k in notes.indices) {
                if (t >= starts[k]) {
                    val dt = t - starts[k]
                    val decay = if (k == 3) 4.2f else 9.5f
                    val c = TWO_PI * notes[k] * dt
                    val m = TWO_PI * notes[k] * 2f * dt
                    out += fm2(c, m, 1.8f * exp(-dt * 8f)) * exp(-dt * decay)
                }
            }
            buf[i] = out * 0.4f
        }
    }

    // 27: Death — ominous descending 3-note dark FM minor chord.
    private fun synthDeath(): ShortArray = makeBuffer(600) { buf ->
        val notes = floatArrayOf(293.66f, 246.94f, 196.00f) // D4 -> B3 -> G3
        val starts = floatArrayOf(0f, 0.16f, 0.32f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            var out = 0f
            for (k in notes.indices) {
                if (t >= starts[k]) {
                    val dt = t - starts[k]
                    val decay = if (k == 2) 3.8f else 7.5f
                    val c = TWO_PI * notes[k] * dt
                    val m = TWO_PI * notes[k] * 1.5f * dt
                    out += (fm2(c, m, 2.4f * exp(-dt * 5f)) + 0.3f * saw(c * 0.5f)) * exp(-dt * decay)
                }
            }
            buf[i] = out * 0.42f
        }
    }

    // 28: Structural breach — deep masonry snap + cascading rubble rumble.
    private fun synthStructuralBreach(): ShortArray = makeBuffer(480) { buf ->
        val rng = XorShift32(0x119)
        val lp = OnePoleLp(620f)
        val bp = SvfBandpass(1100f, 1.8f)
        var ph = 0f
        val n = buf.size
        for (i in 0 until n) {
            val t = i.toFloat() / SAMPLE_RATE
            val u = i.toFloat() / n
            ph += TWO_PI * expSweep(125f, 34f, u) / SAMPLE_RATE
            val crack = sin(ph) * exp(-t * 14f)
            val noise = rng.nextFloat()
            val rumble = lp.process(noise) * exp(-t * 4.8f)
            val debris = bp.process(noise) * (exp(-t * 16f) + 0.55f * exp(-abs(t - 0.16f) * 20f))
            buf[i] = crack * 0.85f + rumble * 0.85f + debris * 0.55f
        }
    }

    // 29: UI click — crisp 28ms tactile retro blip.
    private fun synthUiClick(): ShortArray = makeBuffer(28) { buf ->
        val rng = XorShift32(0x11A)
        val hp = OnePoleHp(1500f)
        for (i in buf.indices) {
            val t = i.toFloat() / SAMPLE_RATE
            val tone = fm2(TWO_PI * 1680f * t, TWO_PI * 3360f * t, 0.9f) * exp(-t * 135f)
            val tick = hp.process(rng.nextFloat()) * exp(-t * 180f)
            buf[i] = tone * 0.78f + tick * 0.35f
        }
    }
}
