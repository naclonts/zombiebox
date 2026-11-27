package game.zombiebox.mobile

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Build
import android.os.Process
import android.os.SystemClock
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLongArray
import java.util.concurrent.locks.LockSupport
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt

/**
 * Process-scoped zero-allocation audio & haptics engine for Zombiebox Android.
 *
 * - Lock-free 64-slot SPSC ring buffer (zero UI-thread blocking, zero allocs in triggerSimEvents)
 * - 12-voice 16.16 fixed-point pitch-varied mixer with smooth master gain ramp & soft-clipper
 * - Single low-latency MODE_STREAM AudioTrack that auto-pauses & parks after ~1.5s of silence
 * - 4-tier Android haptics (TICK, LIGHT, MEDIUM, HEAVY) with API 24-36 compatibility
 */
internal class AudioHapticsEngine private constructor(appContext: Context) {

    private enum class HapticTier(val durationMs: Long, val amplitude: Int) {
        NONE(0L, 0),
        TICK(10L, 55),
        LIGHT(16L, 110),
        MEDIUM(28L, 185),
        HEAVY(48L, 255),
    }

    private val bank: Array<ShortArray> = RetroSynth.buildBank()
    private val engineCycle = RetroSynth.motorcycleEngineCycle()
    private val ridingLevels = AtomicInteger(0)
    private val vibrator: Vibrator? = resolveVibrator(appContext)

    // Settings written on UI thread, read on mixer & UI threads.
    @Volatile
    private var sfxVolumePct: Int = 75
    @Volatile
    private var sfxMuted: Boolean = false
    @Volatile
    private var hapticsEnabled: Boolean = true

    // Lifecycle coordination.
    private val suspended = AtomicBoolean(true)
    private val clearVoicesRequested = AtomicBoolean(false)
    private val discardTargetTail = AtomicInteger(0)
    private val trackLock = Any()

    // Lock-free 64-slot ring buffer (UI producer -> mixer consumer).
    // Each packed Long: bits 0..7 = sfxId, bits 8..23 = gain (Q12), bits 24..39 = pitch (Q12).
    private val ringData = AtomicLongArray(RING_CAPACITY)
    private val ringHead = AtomicInteger(0)
    private val ringTail = AtomicInteger(0)

    // Producer-only anti-spam & deterministic pitch jitter state (UI thread only).
    private val lastTriggerMs = LongArray(RetroSynth.SFX_COUNT)
    private var lastHapticMs = 0L
    private var lastHapticTier = HapticTier.NONE
    private var rngState = 0x5EED_CAFE

    // Mixer thread & AudioTrack (owned by mixer thread; trackLock guards state transitions).
    private var audioTrack: AudioTrack? = null
    private var trackPlaying = false
    private val mixerThread: Thread = Thread(::mixerLoop, "zombiebox-audio").apply {
        isDaemon = true
        start()
    }

    fun setSettings(volumePct: Int, muted: Boolean, haptics: Boolean) {
        sfxVolumePct = volumePct.coerceIn(0, 100)
        sfxMuted = muted
        hapticsEnabled = haptics
    }

    fun resume() {
        suspended.set(false)
    }

    fun suspend() {
        suspended.set(true)
        ridingLevels.set(0)
        // Record tail so mixerLoop (sole writer of ringHead) drops stale pre-suspend triggers.
        discardTargetTail.set(ringTail.get())
        clearVoicesRequested.set(true)
        cancelHaptics()
        synchronized(trackLock) {
            val track = audioTrack
            if (track != null && trackPlaying) {
                try {
                    track.pause()
                    track.flush()
                } catch (_: IllegalStateException) {
                    // Ignore track state races during shutdown.
                }
                trackPlaying = false
            }
        }
    }

    fun setRiding(speedMphX10: Int, load: Int, grip: Int, mounted: Boolean) {
        val levels = if (mounted) {
            1 or (load.coerceIn(0, 100) shl 1) or (grip.coerceIn(0, 100) shl 8) or
                (speedMphX10.coerceIn(0, 65535) shl 15)
        } else 0
        if (ridingLevels.getAndSet(levels) == 0 && levels != 0) LockSupport.unpark(mixerThread)
    }

    fun playUiClick() {
        if (suspended.get()) return
        val now = SystemClock.uptimeMillis()
        enqueueSfx(RetroSynth.SFX_UI_CLICK, 0.72f, 1.0f, now)
        triggerHaptic(HapticTier.TICK, now)
    }

    /**
     * Trigger all SFX and the highest-priority haptic tier encoded in a 64-bit ZB_SFX_* bitmask.
     * Strictly zero allocations and non-blocking on the calling UI thread.
     */
    fun triggerSimEvents(mask: Long) {
        if (mask == 0L || suspended.get()) return
        val now = SystemClock.uptimeMillis()
        var remaining = mask
        var bestHaptic = HapticTier.NONE

        while (remaining != 0L) {
            val sfxId = remaining.countTrailingZeroBits()
            remaining = remaining and (remaining - 1L)
            if (sfxId >= RetroSynth.SFX_UI_CLICK) continue

            val gain = sfxBaseGain(sfxId)
            val pitch = if (sfxAllowsPitchJitter(sfxId)) nextPitchJitter() else 1.0f
            enqueueSfx(sfxId, gain, pitch, now)

            val tier = hapticTierForSfx(sfxId)
            if (tier.ordinal > bestHaptic.ordinal) {
                bestHaptic = tier
            }
        }

        if (bestHaptic != HapticTier.NONE) {
            triggerHaptic(bestHaptic, now)
        }
    }

    private fun enqueueSfx(sfxId: Int, gain: Float, pitch: Float, nowMs: Long) {
        if (sfxMuted || sfxVolumePct <= 0) return
        val cooldown = sfxCooldownMs(sfxId)
        if (nowMs - lastTriggerMs[sfxId] < cooldown) return
        lastTriggerMs[sfxId] = nowMs

        val tail = ringTail.get()
        val head = ringHead.get()
        if (tail - head >= RING_CAPACITY) return

        val gainQ = (gain.coerceIn(0f, 2f) * 4096f).roundToInt() and 0xFFFF
        val pitchQ = (pitch.coerceIn(0.5f, 2f) * 4096f).roundToInt() and 0xFFFF
        val packed = (sfxId.toLong() and 0xFFL) or
            (gainQ.toLong() shl 8) or
            (pitchQ.toLong() shl 24)
        ringData.set(tail and RING_MASK, packed)
        ringTail.set(tail + 1)
        LockSupport.unpark(mixerThread)
    }

    private fun nextPitchJitter(): Float {
        var x = rngState
        x = x xor (x shl 13)
        x = x xor (x ushr 17)
        x = x xor (x shl 5)
        rngState = x
        val unit = ((x and 0x3FF) / 511.5f) - 1f // [-1, 1]
        return 1.0f + unit * 0.06f
    }

    private fun triggerHaptic(tier: HapticTier, nowMs: Long) {
        if (!hapticsEnabled || suspended.get()) return
        val vib = vibrator ?: return
        val elapsed = nowMs - lastHapticMs
        if (elapsed < HAPTIC_COOLDOWN_MS && tier.ordinal <= lastHapticTier.ordinal) return
        lastHapticMs = nowMs
        lastHapticTier = tier
        try {
            if (!vib.hasVibrator()) return
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                vib.vibrate(VibrationEffect.createOneShot(tier.durationMs, tier.amplitude))
            } else {
                @Suppress("DEPRECATION")
                vib.vibrate(tier.durationMs)
            }
        } catch (_: SecurityException) {
            // Permission absent or restricted; continue silently.
        } catch (_: RuntimeException) {
            // Device vibrator service fault; ignore.
        }
    }

    private fun cancelHaptics() {
        val vib = vibrator ?: return
        try {
            vib.cancel()
        } catch (_: RuntimeException) {
            // Ignore vibrator service errors.
        }
    }

    private fun mixerLoop() {
        try {
            Process.setThreadPriority(Process.THREAD_PRIORITY_AUDIO)
        } catch (_: SecurityException) {
            // Continue at normal thread priority if disallowed.
        }

        // Pre-allocated 12-voice mixer state (zero GC pressure).
        val voiceSfx = IntArray(MAX_VOICES) { -1 }
        val voicePosFP = IntArray(MAX_VOICES)
        val voiceStepFP = IntArray(MAX_VOICES)
        val voiceGain = FloatArray(MAX_VOICES)
        val voiceAge = LongArray(MAX_VOICES)
        var voiceSeq = 0L

        val outPcm = ShortArray(CHUNK_SAMPLES)
        var currentMasterGain = if (sfxMuted) 0f else sfxVolumePct / 100f
        var silentSamples = 0
        var enginePhase = 0f
        var engineHz = 24f
        var engineGain = 0f
        var tireGain = 0f
        var tireNoise = 0f
        var noiseState = 0x7534BE91

        while (true) {
            if (clearVoicesRequested.getAndSet(false)) {
                voiceSfx.fill(-1)
                engineGain = 0f
                tireGain = 0f
                val target = discardTargetTail.get()
                val curHead = ringHead.get()
                if (target - curHead > 0) {
                    ringHead.set(target)
                }
                silentSamples = IDLE_SILENCE_SAMPLES
            }

            if (suspended.get()) {
                voiceSfx.fill(-1)
                LockSupport.park()
                continue
            }

            // Drain lock-free trigger ring buffer.
            var head = ringHead.get()
            val tail = ringTail.get()
            while (head != tail) {
                val packed = ringData.get(head and RING_MASK)
                head++
                val sfxId = (packed and 0xFFL).toInt()
                if (sfxId in 0 until RetroSynth.SFX_COUNT) {
                    val gain = ((packed ushr 8) and 0xFFFFL).toFloat() / 4096f
                    val pitch = ((packed ushr 24) and 0xFFFFL).toFloat() / 4096f
                    val slot = allocateVoice(voiceSfx, voiceAge)
                    voiceSfx[slot] = sfxId
                    voicePosFP[slot] = 0
                    voiceStepFP[slot] = (pitch * 65536f).roundToInt().coerceIn(32768, 131072)
                    voiceGain[slot] = gain
                    voiceAge[slot] = ++voiceSeq
                    silentSamples = 0
                }
            }
            ringHead.set(head)

            var activeCount = 0
            for (v in 0 until MAX_VOICES) {
                if (voiceSfx[v] >= 0) activeCount++
            }

            val levels = ridingLevels.get()
            val mounted = levels and 1 != 0
            val load = ((levels ushr 1) and 0x7f) / 100f
            val grip = (levels ushr 8) and 0x7f
            val speed = ((levels ushr 15) / 500f).coerceAtMost(1f)
            val targetHz = 24f + 55f * speed + 31f * load
            val targetEngine = if (mounted) 0.09f + 0.12f * load else 0f
            val targetTire = if (mounted) ((grip - 72).coerceAtLeast(0) / 28f) * speed * 0.26f else 0f
            if (mounted) activeCount++

            if (activeCount == 0 && silentSamples >= IDLE_SILENCE_SAMPLES) {
                pauseTrackIfIdle()
                // Double-check ring before parking to prevent missed wakeups.
                if (ringHead.get() == ringTail.get() && ridingLevels.get() == 0) {
                    LockSupport.park()
                }
                continue
            }

            val track = ensureTrackPlaying()
            if (track == null) {
                voiceSfx.fill(-1)
                LockSupport.parkNanos(200_000_000L)
                continue
            }

            val targetGain = if (sfxMuted) 0f else (sfxVolumePct / 100f)
            for (i in 0 until CHUNK_SAMPLES) {
                currentMasterGain += (targetGain - currentMasterGain) * GAIN_SLEW_ALPHA
                var acc = 0f
                if (mounted || engineGain > 0.001f || tireGain > 0.001f) {
                    engineGain += (targetEngine - engineGain) * GAIN_SLEW_ALPHA
                    tireGain += (targetTire - tireGain) * GAIN_SLEW_ALPHA
                    engineHz += (targetHz - engineHz) * GAIN_SLEW_ALPHA
                    enginePhase += engineHz * engineCycle.size / RetroSynth.SAMPLE_RATE
                    if (enginePhase >= engineCycle.size) enginePhase -= engineCycle.size
                    acc += engineCycle[enginePhase.toInt()] * engineGain
                    noiseState = noiseState xor (noiseState shl 13)
                    noiseState = noiseState xor (noiseState ushr 17)
                    noiseState = noiseState xor (noiseState shl 5)
                    tireNoise += (((noiseState and 0xff) / 127.5f - 1f) - tireNoise) * 0.25f
                    acc += tireNoise * tireGain
                }
                for (v in 0 until MAX_VOICES) {
                    val sfxId = voiceSfx[v]
                    if (sfxId < 0) continue
                    val buf = bank[sfxId]
                    val posFP = voicePosFP[v]
                    val idx = posFP ushr 16
                    if (idx >= buf.size) {
                        voiceSfx[v] = -1
                        continue
                    }
                    val s0 = buf[idx].toFloat()
                    val s1 = if (idx + 1 < buf.size) buf[idx + 1].toFloat() else 0f
                    val frac = (posFP and 0xFFFF) * INV_65536
                    acc += (s0 + (s1 - s0) * frac) * INV_32768 * voiceGain[v]
                    voicePosFP[v] = posFP + voiceStepFP[v]
                }
                val scaled = acc * currentMasterGain
                val softClipped = scaled / (1f + abs(scaled) * 0.45f)
                outPcm[i] = (softClipped * 32767f).roundToInt().coerceIn(-32767, 32767).toShort()
            }

            if (activeCount == 0) {
                silentSamples += CHUNK_SAMPLES
            } else {
                silentSamples = 0
            }

            val written = try {
                track.write(outPcm, 0, CHUNK_SAMPLES, AudioTrack.WRITE_BLOCKING)
            } catch (_: IllegalStateException) {
                -1
            }
            if (written < 0) {
                resetBrokenTrack()
            }
        }
    }

    private fun allocateVoice(voiceSfx: IntArray, voiceAge: LongArray): Int {
        var oldestSlot = 0
        var oldestAge = Long.MAX_VALUE
        for (v in 0 until MAX_VOICES) {
            if (voiceSfx[v] < 0) return v
            if (voiceAge[v] < oldestAge) {
                oldestAge = voiceAge[v]
                oldestSlot = v
            }
        }
        return oldestSlot
    }

    private fun ensureTrackPlaying(): AudioTrack? {
        synchronized(trackLock) {
            if (suspended.get()) return null
            var track = audioTrack
            if (track == null) {
                track = createAudioTrack()
                audioTrack = track
                trackPlaying = false
            }
            if (track != null && !trackPlaying) {
                try {
                    track.play()
                    trackPlaying = true
                } catch (_: IllegalStateException) {
                    resetBrokenTrackLocked()
                    return null
                }
            }
            return track
        }
    }

    private fun pauseTrackIfIdle() {
        synchronized(trackLock) {
            val track = audioTrack
            if (track != null && trackPlaying) {
                try {
                    track.pause()
                    track.flush()
                } catch (_: IllegalStateException) {
                    resetBrokenTrackLocked()
                }
                trackPlaying = false
            }
        }
    }

    private fun resetBrokenTrack() {
        synchronized(trackLock) { resetBrokenTrackLocked() }
    }

    private fun resetBrokenTrackLocked() {
        try {
            audioTrack?.release()
        } catch (_: RuntimeException) {
            // Ignore release errors.
        }
        audioTrack = null
        trackPlaying = false
    }

    private fun createAudioTrack(): AudioTrack? {
        val minBytes = AudioTrack.getMinBufferSize(
            RetroSynth.SAMPLE_RATE,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        if (minBytes <= 0) return null
        val bufferBytes = max(minBytes, CHUNK_SAMPLES * 2 * 2)
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_GAME)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()
        val format = AudioFormat.Builder()
            .setSampleRate(RetroSynth.SAMPLE_RATE)
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            try {
                val track = AudioTrack.Builder()
                    .setAudioAttributes(attrs)
                    .setAudioFormat(format)
                    .setTransferMode(AudioTrack.MODE_STREAM)
                    .setBufferSizeInBytes(bufferBytes)
                    .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                    .build()
                if (track.state == AudioTrack.STATE_INITIALIZED) return track
                track.release()
            } catch (_: RuntimeException) {
                // Fall back to default performance mode.
            }
        }
        return try {
            val track = AudioTrack.Builder()
                .setAudioAttributes(attrs)
                .setAudioFormat(format)
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setBufferSizeInBytes(bufferBytes)
                .build()
            if (track.state == AudioTrack.STATE_INITIALIZED) {
                track
            } else {
                track.release()
                null
            }
        } catch (_: RuntimeException) {
            null
        }
    }

    companion object {
        private const val RING_CAPACITY = 64
        private const val RING_MASK = RING_CAPACITY - 1
        private const val MAX_VOICES = 12
        private const val CHUNK_SAMPLES = 256
        private const val IDLE_SILENCE_SAMPLES = (RetroSynth.SAMPLE_RATE * 3) / 2 // ~1.5s
        private const val GAIN_SLEW_ALPHA = 0.004f
        private const val INV_65536 = 1f / 65536f
        private const val INV_32768 = 1f / 32768f
        private const val HAPTIC_COOLDOWN_MS = 28L

        @Volatile
        private var instance: AudioHapticsEngine? = null

        fun getInstance(context: Context): AudioHapticsEngine =
            instance ?: synchronized(this) {
                instance ?: AudioHapticsEngine(context.applicationContext).also { instance = it }
            }

        private fun resolveVibrator(context: Context): Vibrator? = try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                context.getSystemService(VibratorManager::class.java)?.defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
            }
        } catch (_: RuntimeException) {
            null
        }

        private fun sfxCooldownMs(sfxId: Int): Long = when (sfxId) {
            RetroSynth.SFX_UI_CLICK -> 28L
            RetroSynth.SFX_PISTOL_FIRE, RetroSynth.SFX_AXE_SWING -> 55L
            RetroSynth.SFX_SHOTGUN_FIRE, RetroSynth.SFX_ROCKET_LAUNCH -> 90L
            RetroSynth.SFX_EXPLOSION, RetroSynth.SFX_STRUCTURAL_BREACH -> 85L
            RetroSynth.SFX_MONSTER_ALERT -> 220L
            RetroSynth.SFX_MONSTER_HIT, RetroSynth.SFX_AXE_HIT_FLESH -> 45L
            RetroSynth.SFX_WOOD_SPLINTER, RetroSynth.SFX_STONE_THUD, RetroSynth.SFX_AXE_HIT_WOOD -> 50L
            RetroSynth.SFX_GLASS_SHATTER, RetroSynth.SFX_STREETLIGHT_BREAK -> 65L
            RetroSynth.SFX_DRY_FIRE -> 120L
            RetroSynth.SFX_WIN, RetroSynth.SFX_DEATH -> 400L
            else -> 40L
        }

        private fun sfxBaseGain(sfxId: Int): Float = when (sfxId) {
            RetroSynth.SFX_EXPLOSION -> 1.15f
            RetroSynth.SFX_SHOTGUN_FIRE -> 1.05f
            RetroSynth.SFX_STRUCTURAL_BREACH -> 1.0f
            RetroSynth.SFX_ROCKET_LAUNCH, RetroSynth.SFX_BLOATER_BURST -> 0.95f
            RetroSynth.SFX_PISTOL_FIRE, RetroSynth.SFX_PLAYER_HURT -> 0.90f
            RetroSynth.SFX_AXE_HIT_FLESH, RetroSynth.SFX_GLASS_SHATTER -> 0.85f
            RetroSynth.SFX_MONSTER_ALERT -> 0.65f
            RetroSynth.SFX_STAIRS_STEP, RetroSynth.SFX_WINDOW_CLIMB -> 0.65f
            else -> 0.80f
        }

        private fun sfxAllowsPitchJitter(sfxId: Int): Boolean = when (sfxId) {
            RetroSynth.SFX_UI_CLICK,
            RetroSynth.SFX_PICKUP,
            RetroSynth.SFX_MEDKIT_HEAL,
            RetroSynth.SFX_WIN,
            RetroSynth.SFX_DEATH,
            -> false
            else -> true
        }

        private fun hapticTierForSfx(sfxId: Int): HapticTier = when (sfxId) {
            RetroSynth.SFX_EXPLOSION,
            RetroSynth.SFX_STRUCTURAL_BREACH,
            RetroSynth.SFX_DEATH,
            RetroSynth.SFX_VEHICLE_CRASH,
            -> HapticTier.HEAVY

            RetroSynth.SFX_SHOTGUN_FIRE,
            RetroSynth.SFX_ROCKET_LAUNCH,
            RetroSynth.SFX_PLAYER_HURT,
            RetroSynth.SFX_BLOATER_BURST,
            RetroSynth.SFX_VEHICLE_IMPACT,
            -> HapticTier.MEDIUM

            RetroSynth.SFX_PISTOL_FIRE,
            RetroSynth.SFX_AXE_SWING,
            RetroSynth.SFX_AXE_HIT_FLESH,
            RetroSynth.SFX_AXE_HIT_WOOD,
            RetroSynth.SFX_GLASS_SHATTER,
            RetroSynth.SFX_WOOD_SPLINTER,
            RetroSynth.SFX_STREETLIGHT_BREAK,
            RetroSynth.SFX_MONSTER_HIT,
            RetroSynth.SFX_MONSTER_DOWN,
            RetroSynth.SFX_MEDKIT_HEAL,
            RetroSynth.SFX_WIN,
            RetroSynth.SFX_VEHICLE_SKID,
            -> HapticTier.LIGHT

            RetroSynth.SFX_DRY_FIRE,
            RetroSynth.SFX_RELOAD_START,
            RetroSynth.SFX_RELOAD_FINISH,
            RetroSynth.SFX_DOOR_TOGGLE,
            RetroSynth.SFX_PICKUP,
            RetroSynth.SFX_WEAPON_SWITCH,
            -> HapticTier.TICK

            else -> HapticTier.NONE
        }
    }
}
