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

    @ReactProp(name = "scenario")
    override fun setScenario(view: ZombieboxSurfaceView, scenario: String?) {
        view.scenario = if (scenario == "birch") "birch" else "county"
    }

    @ReactProp(name = "runStarted", defaultBoolean = false)
    override fun setRunStarted(view: ZombieboxSurfaceView, runStarted: Boolean) {
        view.runStarted = runStarted
    }

    @ReactProp(name = "paused")
    override fun setPaused(view: ZombieboxSurfaceView, paused: Boolean) {
        view.paused = paused
    }

    @ReactProp(name = "controlScheme")
    override fun setControlScheme(view: ZombieboxSurfaceView, controlScheme: String?) {
        view.controlScheme = controlScheme ?: "tap_stick"
    }

    @ReactProp(name = "sfxVolumePct", defaultInt = 75)
    override fun setSfxVolumePct(view: ZombieboxSurfaceView, sfxVolumePct: Int) {
        view.sfxVolumePct = sfxVolumePct
    }

    @ReactProp(name = "sfxMuted", defaultBoolean = false)
    override fun setSfxMuted(view: ZombieboxSurfaceView, sfxMuted: Boolean) {
        view.sfxMuted = sfxMuted
    }

    @ReactProp(name = "hapticsEnabled", defaultBoolean = true)
    override fun setHapticsEnabled(view: ZombieboxSurfaceView, hapticsEnabled: Boolean) {
        view.hapticsEnabled = hapticsEnabled
    }

    @ReactProp(name = "aimLookAhead", defaultBoolean = true)
    override fun setAimLookAhead(view: ZombieboxSurfaceView, aimLookAhead: Boolean) {
        view.aimLookAhead = aimLookAhead
    }

    @ReactProp(name = "cameraMode")
    override fun setCameraMode(view: ZombieboxSurfaceView, cameraMode: String?) {
        view.cameraMode = cameraMode ?: "isometric"
    }

    @ReactProp(name = "mountedIsometric", defaultBoolean = false)
    override fun setMountedIsometric(view: ZombieboxSurfaceView, mountedIsometric: Boolean) {
        view.mountedIsometric = mountedIsometric
    }

    override fun openCounty(view: ZombieboxSurfaceView) = view.openCounty()

    override fun requestInteract(view: ZombieboxSurfaceView) = view.requestInteract()

    override fun selectWeapon(view: ZombieboxSurfaceView, slot: Int) = view.selectWeapon(slot)

    override fun requestReload(view: ZombieboxSurfaceView) = view.requestReload()

    override fun requestHeal(view: ZombieboxSurfaceView) = view.requestHeal()

    override fun playUiClick(view: ZombieboxSurfaceView) = view.playUiClick()

    override fun onAfterUpdateTransaction(view: ZombieboxSurfaceView) {
        super.onAfterUpdateTransaction(view)
        view.applyProps()
    }

    override fun getExportedCustomDirectEventTypeConstants(): Map<String, Any> =
        mapOf(
            HudChangedEvent.NAME to mapOf("registrationName" to "onHudChanged"),
            AtlasTeleportEvent.NAME to mapOf("registrationName" to "onAtlasTeleport"),
        )

    companion object {
        const val NAME = "ZombieboxSurface"
    }
}
