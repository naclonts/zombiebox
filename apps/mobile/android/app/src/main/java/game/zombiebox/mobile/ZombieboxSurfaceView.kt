package game.zombiebox.mobile

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.Typeface
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
            current?.let { NativeGame.destroy(it.handle) }
            current = Run(id, seed, NativeGame.create(seed))
        }
        if (owner !== view) {
            owner = view
            NativeGame.resetClock(handle(view))
        }
    }

    fun handle(view: ZombieboxSurfaceView): Long = if (owner === view) current?.handle ?: 0 else 0

    fun detach(view: ZombieboxSurfaceView) {
        if (owner === view) {
            NativeGame.resetClock(handle(view))
            owner = null
        }
    }
}

internal class ZombieboxSurfaceView(private val reactContext: ThemedReactContext) :
    View(reactContext), LifecycleEventListener {
    var runId: String? = null
    var seed: Long? = null
    var paused = true

    private var appliedRunId: String? = null
    private var appliedSeed: Long? = null
    private var resumed = false
    private var drawing = false
    private var useQueued = false
    private var lastHud = -1L

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
    private val move = Stick()
    private val aim = Stick()
    private var tapId = INVALID_POINTER
    private var tapX = 0f
    private var tapY = 0f
    private var tapStart = 0L
    private var tapMoved = false

    init {
        // React Native exposes pause and use as accessible controls above this view.
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    fun applyProps() {
        if (!isAttachedToWindow) return
        val id = runId
        val seedValue = seed
        if (!id.isNullOrEmpty() && seedValue != null && (appliedRunId != id || appliedSeed != seedValue)) {
            stopFrames()
            Runs.detach(this)
            Runs.attach(this, id, seedValue)
            appliedRunId = id
            appliedSeed = seedValue
            lastHud = -1L
            render(System.nanoTime(), false)
        }
        updateFrames()
        invalidate()
    }

    fun requestUse() {
        if (canPlay()) useQueued = true
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
        Runs.detach(this)
        appliedRunId = null
        appliedSeed = null
        super.onDetachedFromWindow()
    }

    override fun onHostResume() {
        resumed = true
        updateFrames()
    }

    override fun onHostPause() {
        resumed = false
        stopFrames()
    }

    override fun onHostDestroy() = onHostPause()

    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        updateFrames()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
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
        move.centerX = margin + left
        move.centerY = y
        aim.centerX = width - margin - right
        aim.centerY = y
    }

    private fun canPlay(): Boolean = isAttachedToWindow && windowVisibility == VISIBLE &&
        hasWindowFocus() && resumed && !paused && buffers.isNotEmpty() && Runs.handle(this) != 0L

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
            if (handle != 0L) NativeGame.resetClock(handle)
            drawing = false
        }
        clearInput()
    }

    private fun clearInput() {
        move.clear()
        aim.clear()
        tapId = INVALID_POINTER
        useQueued = false
        parent?.requestDisallowInterceptTouchEvent(false)
    }

    private fun render(now: Long, advance: Boolean) {
        val handle = Runs.handle(this)
        if (handle == 0L || buffers.isEmpty()) return
        if (!advance) NativeGame.resetClock(handle)
        val nextBuffer = (frontBuffer + 1) % buffers.size
        val hud = NativeGame.frame(
            handle, buffers[nextBuffer], now,
            if (advance) move.x else 0f, if (advance) move.y else 0f,
            if (advance) aim.x else 0f, if (advance) aim.y else 0f,
            advance && useQueued,
        )
        if (advance) useQueued = false
        if (!advance) NativeGame.resetClock(handle)
        frontBuffer = nextBuffer
        emitHud(hud)
        invalidate()
    }

    private fun emitHud(hud: Long) {
        if (hud < 0 || lastHud == hud || id == NO_ID) return
        val dispatcher = UIManagerHelper.getEventDispatcher(reactContext) ?: return
        lastHud = hud
        dispatcher.dispatchEvent(HudChangedEvent(UIManagerHelper.getSurfaceId(reactContext), id, hud))
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
            drawStick(canvas, move, "MOVE")
            drawStick(canvas, aim, "AIM")
        }
    }

    private fun drawStick(canvas: Canvas, stick: Stick, label: String) {
        val x = stick.centerX
        val y = stick.centerY
        val radius = stickRadius
        controlPaint.style = Paint.Style.FILL
        controlPaint.color = 0xA0151C19.toInt()
        canvas.drawCircle(x, y, radius, controlPaint)
        controlPaint.style = Paint.Style.STROKE
        controlPaint.strokeWidth = density
        controlPaint.color = 0xA0E5B96D.toInt()
        canvas.drawCircle(x, y, radius, controlPaint)
        controlPaint.color = 0x40E5B96D
        canvas.drawCircle(x, y, radius * 0.63f, controlPaint)
        canvas.drawLine(x - radius * 0.82f, y, x + radius * 0.82f, y, controlPaint)
        canvas.drawLine(x, y - radius * 0.82f, x, y + radius * 0.82f, controlPaint)

        controlPaint.style = Paint.Style.FILL
        controlPaint.color = if (stick.active) 0xEEE5B96D.toInt() else 0x90E5B96D.toInt()
        canvas.drawCircle(x + stick.x * radius * 0.58f, y + stick.y * radius * 0.58f, radius * 0.24f, controlPaint)
        controlPaint.color = 0xFFF3E4C4.toInt()
        controlPaint.textSize = 10f * density
        controlPaint.typeface = Typeface.MONOSPACE
        controlPaint.textAlign = Paint.Align.CENTER
        canvas.drawText(label, x, y + radius + 16f * density, controlPaint)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!canPlay()) return false
        val index = event.actionIndex
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> touchDown(event, index)
            MotionEvent.ACTION_MOVE -> {
                move.update(event, stickRadius)
                aim.update(event, stickRadius)
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
        val hitRadius = stickRadius + 12f * density
        if (!move.active && move.contains(x, y, hitRadius)) {
            move.take(pointer, x, y, stickRadius)
        } else {
            if (!aim.active && aim.contains(x, y, hitRadius)) aim.take(pointer, x, y, stickRadius)
            if (tapId == INVALID_POINTER) {
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
        if (move.pointer == pointer) move.clear()
        if (aim.pointer == pointer) aim.clear()
        if (tapId == pointer) {
            val movedOnRelease = hypot(event.getX(index) - tapX, event.getY(index) - tapY) > touchSlop
            if (!tapMoved && !movedOnRelease && event.eventTime - tapStart <= TAP_DURATION_MS) performClick()
            tapId = INVALID_POINTER
        }
        if (!move.active && !aim.active && tapId == INVALID_POINTER) {
            parent?.requestDisallowInterceptTouchEvent(false)
        }
    }

    override fun performClick(): Boolean {
        super.performClick()
        requestUse()
        return true
    }

    private class Stick {
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

        fun contains(x: Float, y: Float, radius: Float): Boolean = hypot(x - centerX, y - centerY) <= radius

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

    companion object {
        private const val INVALID_POINTER = -1
        private const val TAP_DURATION_MS = 250L
    }
}

internal class HudChangedEvent(surfaceId: Int, viewId: Int, private val hud: Long) :
    Event<HudChangedEvent>(surfaceId, viewId) {
    override fun getEventName() = NAME

    override fun getEventData() = Arguments.createMap().apply {
        putInt("health", (hud and 0xff).toInt())
        putInt("supplies", ((hud shr 8) and 0xff).toInt())
        putInt("zombies", ((hud shr 16) and 0xff).toInt())
        putBoolean("armed", hud and (1L shl 24) != 0L)
        putBoolean("nearLoot", hud and (1L shl 25) != 0L)
        putBoolean("won", hud and (1L shl 26) != 0L)
        putBoolean("dead", hud and (1L shl 27) != 0L)
    }

    companion object {
        const val NAME = "topHudChanged"
    }
}
