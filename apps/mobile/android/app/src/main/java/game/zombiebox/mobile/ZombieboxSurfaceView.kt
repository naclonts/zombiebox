package game.zombiebox.mobile

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.Typeface
import android.os.SystemClock
import android.view.Choreographer
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.WindowInsets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.LifecycleEventListener
import com.facebook.react.common.LifecycleState
import com.facebook.react.uimanager.ThemedReactContext
import com.facebook.react.uimanager.UIManagerHelper
import com.facebook.react.uimanager.events.Event
import kotlin.math.hypot
import kotlin.math.roundToInt

// A single game survives view replacement. All calls are made on the Android UI thread.
private object Runs {
    private data class Run(val id: String, val seed: Long, val handle: Long)

    private var current: Run? = null
    private var owner: ZombieboxSurfaceView? = null

    fun attach(view: ZombieboxSurfaceView, id: String, seed: Long) {
        if (current?.id != id || current?.seed != seed) {
            current?.let { NativeGame.destroyV2(it.handle) }
            current = Run(id, seed, NativeGame.createV2(seed))
        }
        if (owner !== view) {
            owner = view
            NativeGame.resetClockV2(handle(view))
        }
    }

    fun handle(view: ZombieboxSurfaceView): Long = if (owner === view) current?.handle ?: 0 else 0

    fun detach(view: ZombieboxSurfaceView) {
        if (owner === view) {
            NativeGame.resetClockV2(handle(view))
            owner = null
        }
    }
}

internal enum class ControlScheme {
    TAP_STICK,
    OUTER_RING,
    RELEASE_FIRE,
}

internal class ZombieboxSurfaceView(private val reactContext: ThemedReactContext) :
    View(reactContext), LifecycleEventListener {
    var runId: String? = null
    var seed: Long? = null
    var paused = true
    var controlScheme: String = "tap_stick"
    var sfxVolumePct: Int = 75
    var sfxMuted: Boolean = false
    var hapticsEnabled: Boolean = true
    var aimLookAhead: Boolean = true

    private var appliedRunId: String? = null
    private var appliedSeed: Long? = null
    private var resumed = false
    private var drawing = false

    // One-shot queued discrete inputs (cleared on stopFrames / clearInput).
    private var interactQueued = false
    private var reloadQueued = false
    private var healQueued = false
    private var pendingSelectWeapon = WEAPON_UNCHANGED
    private var currentInteractKind = 0

    // Zero-allocation frame output and HUD diff state.
    private val frameOut = IntArray(12)
    private val lastHudOut = IntArray(9)
    private var hasLastHud = false

    private val audioEngine = AudioHapticsEngine.getInstance(reactContext.applicationContext)

    private val choreographer = Choreographer.getInstance()
    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (canPlay()) {
                render(frameTimeNanos, true)
                choreographer.postFrameCallback(this)
            } else {
                stopFrames()
            }
        }
    }
    private var buffers = emptyArray<Bitmap>()
    private var frontBuffer = 0
    private val gameArea = Rect()
    private val bitmapPaint = Paint().apply { isFilterBitmap = false }
    private val controlPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val density = resources.displayMetrics.density
    private val touchSlop = ViewConfiguration.get(reactContext).scaledTouchSlop.toFloat()
    private var stickRadius = 46f * density

    private val moveStick = MoveStick()
    private val aimStick = AimStickController(density)

    // Misfire-free scene tap (triggers interact ONLY when an interact prompt is active).
    private var tapId = INVALID_POINTER
    private var tapX = 0f
    private var tapY = 0f
    private var tapStart = 0L
    private var tapMoved = false

    init {
        // React Native exposes HUD, hotbar, interact, settings, and pause above this view.
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    fun applyProps() {
        audioEngine.setSettings(sfxVolumePct, sfxMuted, hapticsEnabled)
        aimStick.setScheme(parseScheme(controlScheme))

        if (!isAttachedToWindow) return
        val id = runId
        val seedValue = seed
        if (!id.isNullOrEmpty() && seedValue != null && (appliedRunId != id || appliedSeed != seedValue)) {
            stopFrames()
            Runs.detach(this)
            Runs.attach(this, id, seedValue)
            appliedRunId = id
            appliedSeed = seedValue
            hasLastHud = false
            currentInteractKind = 0
            render(System.nanoTime(), false)
        }
        updateAudioLifecycle()
        updateFrames()
        invalidate()
    }

    fun requestInteract() {
        if (canPlay()) interactQueued = true
    }

    fun selectWeapon(slot: Int) {
        if (canPlay() && slot in 0..3) {
            pendingSelectWeapon = slot
        }
    }

    fun requestReload() {
        if (canPlay()) reloadQueued = true
    }

    fun requestHeal() {
        if (canPlay()) healQueued = true
    }

    fun playUiClick() {
        if (canAudioRun()) {
            audioEngine.playUiClick()
        }
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        resumed = reactContext.lifecycleState == LifecycleState.RESUMED
        reactContext.addLifecycleEventListener(this)
        applyProps()
    }

    override fun onDetachedFromWindow() {
        reactContext.removeLifecycleEventListener(this)
        stopFrames()
        audioEngine.suspend()
        Runs.detach(this)
        appliedRunId = null
        appliedSeed = null
        super.onDetachedFromWindow()
    }

    override fun onHostResume() {
        resumed = true
        updateAudioLifecycle()
        updateFrames()
    }

    override fun onHostPause() {
        resumed = false
        stopFrames()
        updateAudioLifecycle()
    }

    override fun onHostDestroy() = onHostPause()

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        updateAudioLifecycle()
        updateFrames()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        updateAudioLifecycle()
        updateFrames()
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        gameArea.set(0, 0, width, height)
        layoutSticks(ViewCompat.getRootWindowInsets(this))
        if (width <= 0 || height <= 0) {
            buffers = emptyArray()
        } else {
            val scale = minOf(1f, 360f / minOf(width, height), 1200f / maxOf(width, height))
            val pixelWidth = (width * scale).roundToInt().coerceAtLeast(1)
            val pixelHeight = (height * scale).roundToInt().coerceAtLeast(1)
            buffers = Array(2) {
                Bitmap.createBitmap(pixelWidth, pixelHeight, Bitmap.Config.ARGB_8888).apply {
                    setHasAlpha(false)
                }
            }
            frontBuffer = 0
            render(System.nanoTime(), false)
        }
        updateFrames()
    }

    override fun onApplyWindowInsets(insets: WindowInsets): WindowInsets {
        layoutSticks(WindowInsetsCompat.toWindowInsetsCompat(insets, this))
        return super.onApplyWindowInsets(insets)
    }

    private fun layoutSticks(insets: WindowInsetsCompat?) {
        val sides = insets?.getInsets(WindowInsetsCompat.Type.displayCutout())
        val gestures = insets?.getInsets(WindowInsetsCompat.Type.mandatorySystemGestures())
        stickRadius = minOf(46f * density, width * 0.15f, height * 0.20f)
        val margin = stickRadius + 32f * density
        val left = (sides?.left ?: 0).toFloat()
        val right = (sides?.right ?: 0).toFloat()
        val bottom = maxOf(sides?.bottom ?: 0, gestures?.bottom ?: 0).toFloat()
        val y = height - bottom - stickRadius - 33f * density
        moveStick.centerX = margin + left
        moveStick.centerY = y
        aimStick.centerX = width - margin - right
        aimStick.centerY = y
    }

    private fun canAudioRun(): Boolean =
        isAttachedToWindow && windowVisibility == VISIBLE && hasWindowFocus() && resumed

    private fun canPlay(): Boolean =
        canAudioRun() && !paused && buffers.isNotEmpty() && Runs.handle(this) != 0L

    private fun updateAudioLifecycle() {
        if (canAudioRun()) {
            audioEngine.resume()
        } else {
            audioEngine.suspend()
        }
    }

    private fun updateFrames() {
        if (canPlay()) {
            if (!drawing) {
                drawing = true
                choreographer.postFrameCallback(frameCallback)
            }
        } else {
            stopFrames()
        }
    }

    private fun stopFrames() {
        if (drawing) {
            choreographer.removeFrameCallback(frameCallback)
            val handle = Runs.handle(this)
            if (handle != 0L) NativeGame.resetClockV2(handle)
            drawing = false
        }
        clearInput()
    }

    private fun clearInput() {
        moveStick.clear()
        aimStick.clear()
        tapId = INVALID_POINTER
        interactQueued = false
        reloadQueued = false
        healQueued = false
        pendingSelectWeapon = WEAPON_UNCHANGED
        parent?.requestDisallowInterceptTouchEvent(false)
    }

    private fun render(now: Long, advance: Boolean) {
        val handle = Runs.handle(this)
        if (handle == 0L || buffers.isEmpty()) return
        if (!advance) NativeGame.resetClockV2(handle)

        val nowMs = SystemClock.uptimeMillis()
        var actionFlags = if (aimLookAhead) INPUT_LOOK_AHEAD else 0
        var selectWeapon = WEAPON_UNCHANGED
        val aimX: Float
        val aimY: Float
        if (advance) {
            aimStick.prepareFrameAim()
            aimX = aimStick.frameAimX
            aimY = aimStick.frameAimY
            if (aimStick.consumeFirePulse()) actionFlags = actionFlags or INPUT_FIRE_PULSE
            if (aimStick.isFireHold(nowMs)) actionFlags = actionFlags or INPUT_FIRE_HOLD
            if (interactQueued) actionFlags = actionFlags or INPUT_INTERACT
            if (reloadQueued) actionFlags = actionFlags or INPUT_RELOAD
            if (healQueued) actionFlags = actionFlags or INPUT_HEAL
            selectWeapon = pendingSelectWeapon
        } else {
            aimX = 0f
            aimY = 0f
        }

        val nextBuffer = (frontBuffer + 1) % buffers.size
        val ok = NativeGame.frameV2(
            handle,
            buffers[nextBuffer],
            now,
            if (advance) moveStick.x else 0f,
            if (advance) moveStick.y else 0f,
            aimX,
            aimY,
            actionFlags,
            selectWeapon,
            frameOut,
        )
        if (advance) {
            interactQueued = false
            reloadQueued = false
            healQueued = false
            pendingSelectWeapon = WEAPON_UNCHANGED
        } else {
            NativeGame.resetClockV2(handle)
        }

        if (ok) {
            frontBuffer = nextBuffer
            currentInteractKind = (frameOut[6] ushr 16) and 0xff
            if (advance) {
                val audioMask = (frameOut[9].toLong() and 0xFFFF_FFFFL) or
                    (frameOut[10].toLong() shl 32)
                if (audioMask != 0L) {
                    audioEngine.triggerSimEvents(audioMask)
                }
            }
            emitHudIfChanged()
        }
        invalidate()
    }

    private fun emitHudIfChanged() {
        if (id == NO_ID) return
        var changed = !hasLastHud
        if (!changed) {
            for (i in 0 until 9) {
                if (frameOut[i] != lastHudOut[i]) {
                    changed = true
                    break
                }
            }
        }
        if (!changed) return
        val dispatcher = UIManagerHelper.getEventDispatcher(reactContext) ?: return
        for (i in 0 until 9) {
            lastHudOut[i] = frameOut[i]
        }
        hasLastHud = true
        dispatcher.dispatchEvent(
            HudChangedEvent(
                UIManagerHelper.getSurfaceId(reactContext),
                id,
                frameOut.copyOf(9),
            ),
        )
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val bitmap = buffers.getOrNull(frontBuffer)
        if (bitmap != null) {
            canvas.drawBitmap(bitmap, null, gameArea, bitmapPaint)
        } else {
            canvas.drawColor(Color.rgb(21, 28, 25))
        }
        if (!paused) {
            val nowMs = SystemClock.uptimeMillis()
            drawMoveStick(canvas)
            drawAimStick(canvas, nowMs)
        }
    }

    private fun drawMoveStick(canvas: Canvas) {
        val x = moveStick.centerX
        val y = moveStick.centerY
        val radius = stickRadius
        controlPaint.style = Paint.Style.FILL
        controlPaint.color = 0xB5171D22.toInt()
        canvas.drawCircle(x, y, radius, controlPaint)

        controlPaint.style = Paint.Style.STROKE
        controlPaint.strokeWidth = density
        controlPaint.color = if (moveStick.active) 0xFFE1AE63.toInt() else 0xC79FABB0.toInt()
        canvas.drawCircle(x, y, radius, controlPaint)

        // Subtle inner quiet-walk threshold ring (0.52).
        controlPaint.color = 0x559FABB0
        canvas.drawCircle(x, y, radius * 0.52f, controlPaint)
        canvas.drawLine(x - radius * 0.82f, y, x + radius * 0.82f, y, controlPaint)
        canvas.drawLine(x, y - radius * 0.82f, x, y + radius * 0.82f, controlPaint)

        controlPaint.style = Paint.Style.FILL
        controlPaint.color = if (moveStick.active) 0xFFE1AE63.toInt() else 0xABBEBBB3.toInt()
        canvas.drawCircle(
            x + moveStick.x * radius * 0.58f,
            y + moveStick.y * radius * 0.58f,
            radius * 0.24f,
            controlPaint,
        )

        drawStickLabel(canvas, "MOVE", x, y + radius + 16f * density, 0xFFF1EAE0.toInt())
    }

    private fun drawAimStick(canvas: Canvas, nowMs: Long) {
        val x = aimStick.centerX
        val y = aimStick.centerY
        val radius = stickRadius
        val firing = aimStick.isFiringVisual(nowMs)
        val flashing = aimStick.isFlashing(nowMs)
        val cancelled = aimStick.isReleaseCancelled()

        controlPaint.style = Paint.Style.FILL
        controlPaint.color = if (flashing) 0xCC2C2418.toInt() else 0xB5171D22.toInt()
        canvas.drawCircle(x, y, radius, controlPaint)

        // Outer border.
        controlPaint.style = Paint.Style.STROKE
        controlPaint.strokeWidth = if (firing || flashing) 2.2f * density else density
        controlPaint.color = when {
            firing || flashing -> 0xFFFF9433.toInt()
            cancelled -> 0x998C9499.toInt()
            aimStick.active -> 0xFFE1AE63.toInt()
            else -> 0xC79FABB0.toInt()
        }
        canvas.drawCircle(x, y, radius, controlPaint)

        // Scheme-specific inner rings & crosshairs.
        controlPaint.strokeWidth = density
        when (aimStick.scheme) {
            ControlScheme.OUTER_RING -> {
                controlPaint.color = 0x559FABB0
                canvas.drawLine(x - radius * 0.72f, y, x + radius * 0.72f, y, controlPaint)
                canvas.drawLine(x, y - radius * 0.72f, x, y + radius * 0.72f, controlPaint)

                // Crisp amber fire ring at 78% radius (glows orange-amber when firing).
                controlPaint.strokeWidth = if (firing) 2.4f * density else 1.5f * density
                controlPaint.color = if (firing) 0xFFFF8A24.toInt() else 0xCCE1AE63.toInt()
                canvas.drawCircle(x, y, radius * OUTER_RING_ENTER, controlPaint)
            }
            ControlScheme.RELEASE_FIRE -> {
                controlPaint.color = if (cancelled) 0x99D96B62.toInt() else 0x649FABB0
                canvas.drawCircle(x, y, radius * RELEASE_DEADZONE, controlPaint)
                controlPaint.color = 0x559FABB0
                canvas.drawLine(x - radius * 0.82f, y, x + radius * 0.82f, y, controlPaint)
                canvas.drawLine(x, y - radius * 0.82f, x, y + radius * 0.82f, controlPaint)
            }
            ControlScheme.TAP_STICK -> {
                controlPaint.color = if (flashing) 0xFFE1AE63.toInt() else 0x649FABB0
                canvas.drawCircle(x, y, radius * 0.63f, controlPaint)
                controlPaint.color = 0x559FABB0
                canvas.drawLine(x - radius * 0.82f, y, x + radius * 0.82f, y, controlPaint)
                canvas.drawLine(x, y - radius * 0.82f, x, y + radius * 0.82f, controlPaint)
            }
        }

        // Thumb knob.
        controlPaint.style = Paint.Style.FILL
        controlPaint.color = when {
            firing || flashing -> 0xFFFF9A3C.toInt()
            cancelled -> 0x788A9096
            aimStick.active -> 0xFFE1AE63.toInt()
            else -> 0xABBEBBB3.toInt()
        }
        val knobTravel = if (aimStick.scheme == ControlScheme.OUTER_RING) 0.78f else 0.58f
        canvas.drawCircle(
            x + aimStick.x * radius * knobTravel,
            y + aimStick.y * radius * knobTravel,
            radius * 0.24f,
            controlPaint,
        )

        val label = aimStick.label(nowMs)
        val labelColor = when {
            cancelled -> 0xFFCC9A94.toInt()
            firing || flashing -> 0xFFFFB25C.toInt()
            else -> 0xFFF1EAE0.toInt()
        }
        drawStickLabel(canvas, label, x, y + radius + 16f * density, labelColor)
    }

    private fun drawStickLabel(canvas: Canvas, text: String, x: Float, y: Float, color: Int) {
        controlPaint.style = Paint.Style.FILL
        controlPaint.color = color
        controlPaint.textSize = 10f * density
        controlPaint.typeface = Typeface.MONOSPACE
        controlPaint.textAlign = Paint.Align.CENTER
        canvas.drawText(text, x, y, controlPaint)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!canPlay()) return false
        val index = event.actionIndex
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> touchDown(event, index)
            MotionEvent.ACTION_MOVE -> {
                moveStick.update(event, stickRadius)
                aimStick.update(event, stickRadius)
                val tapIndex = event.findPointerIndex(tapId)
                if (tapIndex >= 0) {
                    tapMoved = tapMoved || hypot(event.getX(tapIndex) - tapX, event.getY(tapIndex) - tapY) > touchSlop
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> touchUp(event, index)
            MotionEvent.ACTION_CANCEL -> clearInput()
        }
        return true
    }

    private fun touchDown(event: MotionEvent, index: Int) {
        val pointer = event.getPointerId(index)
        val x = event.getX(index)
        val y = event.getY(index)
        val moveHitRadius = stickRadius + 14f * density
        val aimHitRadius = stickRadius + 16f * density
        when {
            moveStick.contains(x, y, moveHitRadius) -> {
                if (!moveStick.active) moveStick.take(pointer, x, y, stickRadius)
            }
            aimStick.contains(x, y, aimHitRadius) -> {
                if (!aimStick.active) aimStick.down(pointer, x, y, event.eventTime, stickRadius)
            }
            tapId == INVALID_POINTER -> {
                tapId = pointer
                tapX = x
                tapY = y
                tapStart = event.eventTime
                tapMoved = false
            }
        }
        parent?.requestDisallowInterceptTouchEvent(true)
    }

    private fun touchUp(event: MotionEvent, index: Int) {
        val pointer = event.getPointerId(index)
        if (moveStick.pointer == pointer) moveStick.clear()
        if (aimStick.pointer == pointer) {
            aimStick.up(event.getX(index), event.getY(index), event.eventTime, stickRadius)
        }
        if (tapId == pointer) {
            val movedOnRelease = hypot(event.getX(index) - tapX, event.getY(index) - tapY) > touchSlop
            // Misfire-free scene tap: NEVER fire; interact ONLY when a contextual prompt is active.
            if (!tapMoved &&
                !movedOnRelease &&
                event.eventTime - tapStart <= TAP_DURATION_MS &&
                currentInteractKind != 0
            ) {
                performClick()
            }
            tapId = INVALID_POINTER
        }
        if (!moveStick.active && !aimStick.active && tapId == INVALID_POINTER) {
            parent?.requestDisallowInterceptTouchEvent(false)
        }
    }

    override fun performClick(): Boolean {
        super.performClick()
        if (currentInteractKind != 0) {
            requestInteract()
        }
        return true
    }

    private class MoveStick {
        var centerX = 0f
        var centerY = 0f
        var pointer = INVALID_POINTER
            private set
        var x = 0f
            private set
        var y = 0f
            private set
        private var startX = 0f
        private var startY = 0f
        val active get() = pointer != INVALID_POINTER

        fun contains(x: Float, y: Float, radius: Float): Boolean =
            hypot(x - centerX, y - centerY) <= radius

        fun take(pointer: Int, x: Float, y: Float, radius: Float) {
            this.pointer = pointer
            startX = x
            startY = y
            update(x, y, radius)
        }

        fun update(event: MotionEvent, radius: Float) {
            val index = event.findPointerIndex(pointer)
            if (index >= 0) update(event.getX(index), event.getY(index), radius)
        }

        private fun update(x: Float, y: Float, radius: Float) {
            val dx = x - startX
            val dy = y - startY
            val length = hypot(dx, dy)
            val deadZone = radius * 0.17f
            if (length <= deadZone) {
                this.x = 0f
                this.y = 0f
            } else {
                val magnitude = ((length - deadZone) / (radius - deadZone)).coerceAtMost(1f)
                this.x = dx / length * magnitude
                this.y = dy / length * magnitude
            }
        }

        fun clear() {
            pointer = INVALID_POINTER
            x = 0f
            y = 0f
        }
    }

    /**
     * Right-stick controller supporting 3 configurable twin-stick control schemes:
     * - TAP_STICK (default): floating origin; quick tap fires along retained aim; drag aims;
     *   double-tap-and-hold enters continuous fireHold with gentle aim steering.
     * - OUTER_RING: fixed stick center; inner 78% aims; pushing past 78% (hysteresis exit 70%,
     *   40ms edge-land grace) auto-fires continuously.
     * - RELEASE_FIRE: floating origin; drag past deadzone arms aim and release fires;
     *   pulling back inside deadzone cancels safely; quick tap fires along retained aim.
     */
    private class AimStickController(private val density: Float) {
        private enum class ReleaseState { IDLE, TOUCHED, ARMED, CANCELLED }

        var centerX = 0f
        var centerY = 0f
        var scheme: ControlScheme = ControlScheme.TAP_STICK
            private set
        var pointer = INVALID_POINTER
            private set
        var x = 0f
            private set
        var y = 0f
            private set

        val active get() = pointer != INVALID_POINTER

        private var downX = 0f
        private var downY = 0f
        private var downTime = 0L
        private var maxTravel = 0f

        // Tap-stick double-tap-and-hold state.
        private var lastQuickTapUpTime = 0L
        private var doubleTapCandidate = false
        private var holdEntered = false
        private var dragging = false

        // Outer-ring state.
        private var outerFiring = false
        private var inOuterZone = false
        private var landedInOuter = false

        // Release-fire state.
        private var releaseState = ReleaseState.IDLE

        // Output fire pulse + brief visual ring flash + flick-release aim preservation.
        private var firePulse = false
        private var flashUntilMs = 0L
        private var pendingAimX = 0f
        private var pendingAimY = 0f
        var frameAimX = 0f
            private set
        var frameAimY = 0f
            private set

        fun setScheme(newScheme: ControlScheme) {
            if (scheme != newScheme) {
                scheme = newScheme
                clear()
            }
        }

        fun contains(px: Float, py: Float, radius: Float): Boolean =
            hypot(px - centerX, py - centerY) <= radius

        fun down(pointer: Int, px: Float, py: Float, eventTime: Long, radius: Float) {
            this.pointer = pointer
            downX = px
            downY = py
            downTime = eventTime
            maxTravel = 0f
            x = 0f
            y = 0f
            holdEntered = false
            dragging = false
            outerFiring = false
            inOuterZone = false
            landedInOuter = false

            when (scheme) {
                ControlScheme.TAP_STICK -> {
                    doubleTapCandidate =
                        lastQuickTapUpTime > 0L && (eventTime - lastQuickTapUpTime) in 0L..DOUBLE_TAP_GAP_MS
                }
                ControlScheme.OUTER_RING -> {
                    val normDist = hypot(px - centerX, py - centerY) / radius
                    landedInOuter = normDist >= OUTER_RING_ENTER
                    updateOuterRing(px, py, eventTime, radius)
                }
                ControlScheme.RELEASE_FIRE -> {
                    releaseState = ReleaseState.TOUCHED
                }
            }
        }

        fun update(event: MotionEvent, radius: Float) {
            val index = event.findPointerIndex(pointer)
            if (index < 0) return
            val px = event.getX(index)
            val py = event.getY(index)
            val eventTime = event.eventTime

            when (scheme) {
                ControlScheme.TAP_STICK -> updateTapStick(px, py, eventTime, radius)
                ControlScheme.OUTER_RING -> updateOuterRing(px, py, eventTime, radius)
                ControlScheme.RELEASE_FIRE -> updateReleaseFire(px, py, radius)
            }
        }

        private fun updateTapStick(px: Float, py: Float, eventTime: Long, radius: Float) {
            val dx = px - downX
            val dy = py - downY
            val dist = hypot(dx, dy)
            if (dist > maxTravel) maxTravel = dist

            val dragThreshold = dragThresholdPx(radius)
            if (doubleTapCandidate && !holdEntered && eventTime - downTime >= DOUBLE_TAP_HOLD_MS) {
                holdEntered = true
            }

            if (holdEntered) {
                // Gentle aim steering while holding continuous fire.
                applyAnalogVector(dx, dy, dist, radius * 0.17f, radius)
            } else if (dist > dragThreshold) {
                dragging = true
                doubleTapCandidate = false
                applyAnalogVector(dx, dy, dist, radius * 0.17f, radius)
            } else if (dragging) {
                applyAnalogVector(dx, dy, dist, radius * 0.17f, radius)
            } else {
                x = 0f
                y = 0f
            }
        }

        private fun updateOuterRing(px: Float, py: Float, eventTime: Long, radius: Float) {
            val dx = px - centerX
            val dy = py - centerY
            val dist = hypot(dx, dy)
            val normDist = dist / radius
            applyAnalogVector(dx, dy, dist, radius * 0.17f, radius)

            if (normDist < OUTER_RING_ENTER) {
                landedInOuter = false
            }
            if (outerFiring) {
                if (normDist < OUTER_RING_EXIT) {
                    outerFiring = false
                    inOuterZone = false
                }
            } else {
                inOuterZone = normDist >= OUTER_RING_ENTER
                if (inOuterZone && (!landedInOuter || eventTime - downTime >= OUTER_EDGE_GRACE_MS)) {
                    outerFiring = true
                }
            }
        }

        private fun updateReleaseFire(px: Float, py: Float, radius: Float) {
            val dx = px - downX
            val dy = py - downY
            val dist = hypot(dx, dy)
            if (dist > maxTravel) maxTravel = dist

            val deadZone = radius * RELEASE_DEADZONE
            if (dist > deadZone) {
                releaseState = ReleaseState.ARMED
                applyAnalogVector(dx, dy, dist, deadZone, radius)
            } else {
                x = 0f
                y = 0f
                if (releaseState == ReleaseState.ARMED) {
                    releaseState = ReleaseState.CANCELLED
                }
            }
        }

        fun up(px: Float, py: Float, eventTime: Long, radius: Float) {
            val dist = hypot(px - downX, py - downY)
            if (dist > maxTravel) maxTravel = dist
            val duration = eventTime - downTime
            val dragThreshold = dragThresholdPx(radius)

            when (scheme) {
                ControlScheme.TAP_STICK -> {
                    if (!holdEntered && maxTravel <= dragThreshold && duration <= QUICK_TAP_MAX_MS) {
                        firePulse = true
                        flashUntilMs = SystemClock.uptimeMillis() + RING_FLASH_MS
                        lastQuickTapUpTime = eventTime
                    } else {
                        lastQuickTapUpTime = 0L
                    }
                }
                ControlScheme.OUTER_RING -> {
                    // Release stops auto-fire cleanly.
                }
                ControlScheme.RELEASE_FIRE -> {
                    if (releaseState == ReleaseState.ARMED) {
                        firePulse = true
                        pendingAimX = x
                        pendingAimY = y
                        flashUntilMs = SystemClock.uptimeMillis() + RING_FLASH_MS
                    } else if (releaseState == ReleaseState.TOUCHED &&
                        maxTravel <= dragThreshold &&
                        duration <= QUICK_TAP_MAX_MS
                    ) {
                        // Quick tap fires along retained aim.
                        firePulse = true
                        flashUntilMs = SystemClock.uptimeMillis() + RING_FLASH_MS
                    }
                }
            }

            pointer = INVALID_POINTER
            x = 0f
            y = 0f
            doubleTapCandidate = false
            holdEntered = false
            dragging = false
            outerFiring = false
            inOuterZone = false
            landedInOuter = false
            releaseState = ReleaseState.IDLE
        }

        fun prepareFrameAim() {
            if (active && (x != 0f || y != 0f)) {
                pendingAimX = 0f
                pendingAimY = 0f
                frameAimX = x
                frameAimY = y
            } else if (pendingAimX != 0f || pendingAimY != 0f) {
                frameAimX = pendingAimX
                frameAimY = pendingAimY
                pendingAimX = 0f
                pendingAimY = 0f
            } else {
                frameAimX = 0f
                frameAimY = 0f
            }
        }

        fun consumeFirePulse(): Boolean {
            val pulse = firePulse
            firePulse = false
            return pulse
        }

        fun isFireHold(nowMs: Long): Boolean {
            if (!active) return false
            return when (scheme) {
                ControlScheme.TAP_STICK -> {
                    if (doubleTapCandidate && !holdEntered && nowMs - downTime >= DOUBLE_TAP_HOLD_MS) {
                        holdEntered = true
                    }
                    holdEntered
                }
                ControlScheme.OUTER_RING -> {
                    if (!outerFiring && inOuterZone && landedInOuter && nowMs - downTime >= OUTER_EDGE_GRACE_MS) {
                        outerFiring = true
                    }
                    outerFiring
                }
                ControlScheme.RELEASE_FIRE -> false
            }
        }

        fun isFiringVisual(nowMs: Long): Boolean = isFireHold(nowMs)

        fun isFlashing(nowMs: Long): Boolean = nowMs < flashUntilMs

        fun isReleaseCancelled(): Boolean =
            scheme == ControlScheme.RELEASE_FIRE && active && releaseState == ReleaseState.CANCELLED

        fun label(nowMs: Long): String = when (scheme) {
            ControlScheme.TAP_STICK -> if (isFireHold(nowMs)) "FIRE" else "AIM · TAP"
            ControlScheme.OUTER_RING -> "FIRE RING"
            ControlScheme.RELEASE_FIRE -> when {
                !active -> "AIM · REL"
                releaseState == ReleaseState.CANCELLED -> "CANCEL"
                releaseState == ReleaseState.ARMED -> "RELEASE"
                else -> "AIM · REL"
            }
        }

        fun clear() {
            pointer = INVALID_POINTER
            x = 0f
            y = 0f
            doubleTapCandidate = false
            holdEntered = false
            dragging = false
            lastQuickTapUpTime = 0L
            outerFiring = false
            inOuterZone = false
            landedInOuter = false
            releaseState = ReleaseState.IDLE
            firePulse = false
            flashUntilMs = 0L
            pendingAimX = 0f
            pendingAimY = 0f
            frameAimX = 0f
            frameAimY = 0f
        }

        private fun dragThresholdPx(radius: Float): Float =
            maxOf(radius * 0.20f, 10f * density)

        private fun applyAnalogVector(dx: Float, dy: Float, dist: Float, deadZone: Float, radius: Float) {
            if (dist <= deadZone) {
                x = 0f
                y = 0f
            } else {
                val magnitude = ((dist - deadZone) / (radius - deadZone)).coerceAtMost(1f)
                x = dx / dist * magnitude
                y = dy / dist * magnitude
            }
        }
    }

    companion object {
        private const val INVALID_POINTER = -1
        private const val WEAPON_UNCHANGED = -1
        private const val TAP_DURATION_MS = 250L
        private const val QUICK_TAP_MAX_MS = 220L
        private const val DOUBLE_TAP_GAP_MS = 280L
        private const val DOUBLE_TAP_HOLD_MS = 200L
        private const val RING_FLASH_MS = 90L
        private const val OUTER_RING_ENTER = 0.78f
        private const val OUTER_RING_EXIT = 0.70f
        private const val OUTER_EDGE_GRACE_MS = 40L
        private const val RELEASE_DEADZONE = 0.22f

        private const val INPUT_FIRE_PULSE = 1 shl 0
        private const val INPUT_FIRE_HOLD = 1 shl 1
        private const val INPUT_INTERACT = 1 shl 2
        private const val INPUT_RELOAD = 1 shl 3
        private const val INPUT_HEAL = 1 shl 4
        private const val INPUT_LOOK_AHEAD = 1 shl 5

        private fun parseScheme(value: String?): ControlScheme = when (value) {
            "outer_ring" -> ControlScheme.OUTER_RING
            "release_fire" -> ControlScheme.RELEASE_FIRE
            else -> ControlScheme.TAP_STICK
        }
    }
}

internal class HudChangedEvent(surfaceId: Int, viewId: Int, private val words: IntArray) :
    Event<HudChangedEvent>(surfaceId, viewId) {
    override fun getEventName() = NAME

    override fun getEventData() = Arguments.createMap().apply {
        val w0 = words[0]
        val w1 = words[1]
        val w2 = words[2]
        val w3 = words[3]
        val w4 = words[4]
        val w5 = words[5]
        val w6 = words[6]
        val flags = words[7] and 0xffff

        val equippedRaw = (w0 ushr 24) and 0xff
        val equippedWeapon = if (equippedRaw == 255) -1 else equippedRaw

        putInt("health", w0 and 0xff)
        putInt("maxHealth", (w0 ushr 8) and 0xff)
        putInt("floor", (w0 ushr 16) and 0xff)
        putInt("equippedWeapon", equippedWeapon)
        putInt("ownedWeaponsMask", w1 and 0xff)
        putInt("activeMag", (w1 ushr 8) and 0xff)
        putInt("activeMagCap", (w1 ushr 16) and 0xff)
        putInt("reloadProgress", (w1 ushr 24) and 0xff)
        putInt("activeReserve", w2 and 0xffff)
        putInt("pistolReserve", (w2 ushr 16) and 0xffff)
        putInt("shotgunReserve", w3 and 0xffff)
        putInt("rocketReserve", (w3 ushr 16) and 0xffff)
        putInt("pistolMag", w4 and 0xff)
        putInt("shotgunMag", (w4 ushr 8) and 0xff)
        putInt("rocketMag", (w4 ushr 16) and 0xff)
        putInt("medkits", (w4 ushr 24) and 0xff)
        putInt("zombies", w5 and 0xffff)
        putInt("zombiesTotal", (w5 ushr 16) and 0xffff)
        putInt("cachesFound", w6 and 0xff)
        putInt("cachesTotal", (w6 ushr 8) and 0xff)
        putInt("interactKind", (w6 ushr 16) and 0xff)
        putInt("objectiveKind", (w6 ushr 24) and 0xff)
        putBoolean("won", (flags and (1 shl 0)) != 0)
        putBoolean("dead", (flags and (1 shl 1)) != 0)
        putBoolean("reloading", (flags and (1 shl 2)) != 0)
        putBoolean("canReload", (flags and (1 shl 3)) != 0)
        putBoolean("canHeal", (flags and (1 shl 4)) != 0)
        putBoolean("lowAmmo", (flags and (1 shl 5)) != 0)
        putBoolean("indoors", (flags and (1 shl 6)) != 0)
        putBoolean("dangerClose", (flags and (1 shl 7)) != 0)
    }

    companion object {
        const val NAME = "topHudChanged"
    }
}
