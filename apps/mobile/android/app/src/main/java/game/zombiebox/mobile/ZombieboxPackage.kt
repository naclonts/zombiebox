package game.zombiebox.mobile

import com.facebook.react.ReactPackage
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.module.annotations.ReactModule
import com.facebook.react.uimanager.SimpleViewManager
import com.facebook.react.uimanager.ThemedReactContext
import com.facebook.react.uimanager.ViewManager
import com.facebook.react.uimanager.ViewManagerDelegate
import com.facebook.react.uimanager.annotations.ReactProp
import com.facebook.react.viewmanagers.ZombieboxSurfaceManagerDelegate
import com.facebook.react.viewmanagers.ZombieboxSurfaceManagerInterface

class ZombieboxPackage : ReactPackage {
    override fun createViewManagers(reactContext: ReactApplicationContext): List<ViewManager<*, *>> =
        listOf(ZombieboxSurfaceManager())
}

@ReactModule(name = ZombieboxSurfaceManager.NAME)
internal class ZombieboxSurfaceManager : SimpleViewManager<ZombieboxSurfaceView>(),
    ZombieboxSurfaceManagerInterface<ZombieboxSurfaceView> {
    private val delegate = ZombieboxSurfaceManagerDelegate(this)

    override fun getName() = NAME

    override fun getDelegate(): ViewManagerDelegate<ZombieboxSurfaceView> = delegate

    override fun createViewInstance(context: ThemedReactContext) = ZombieboxSurfaceView(context)

    @ReactProp(name = "runId")
    override fun setRunId(view: ZombieboxSurfaceView, runId: String?) {
        view.runId = runId
    }

    @ReactProp(name = "seed")
    override fun setSeed(view: ZombieboxSurfaceView, seed: String?) {
        view.seed = seed?.toULongOrNull()?.toLong()
    }

    @ReactProp(name = "paused")
    override fun setPaused(view: ZombieboxSurfaceView, paused: Boolean) {
        view.paused = paused
    }

    override fun requestUse(view: ZombieboxSurfaceView) = view.requestUse()

    override fun onAfterUpdateTransaction(view: ZombieboxSurfaceView) {
        super.onAfterUpdateTransaction(view)
        view.applyProps()
    }

    override fun getExportedCustomDirectEventTypeConstants(): Map<String, Any> =
        mapOf(HudChangedEvent.NAME to mapOf("registrationName" to "onHudChanged"))

    companion object {
        const val NAME = "ZombieboxSurface"
    }
}
