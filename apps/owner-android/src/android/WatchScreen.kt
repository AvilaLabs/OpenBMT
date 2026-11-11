package org.openbmt.owner.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.view.View
import java.security.SecureRandom
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.OwnerWatch
import org.openbmt.owner.SettingsQueued

// "Watch in the background" (OA-80 to OA-89): the setting's screen, turning it on and turning it off. The watch
// key's secret is drawn here and sealed in its own file; naming the key in the settings goes through the same
// queue and the same Key app approval as every other change of the settings (OA-34, OA-35). This file signs nothing.

/** The setting's screen: the words, whether it is on, and one button for the other state. [message] is what the last step said. */
internal fun Controller.showWatch(message: SayWords? = null) {
    val on = watchStore.enabled()
    val views = ArrayList<View>()
    views.add(para(AppWords.watchSettingMore))
    views.add(ui.text(if (on) "On" else "Off", 16f, bold = true))
    message?.let { views.add(ui.words(it)) }
    val actions = ArrayList<android.widget.Button>()
    if (on) {
        actions.add(ui.danger("Turn off") { watchTurnOff() })
    } else {
        actions.add(ui.primary("Turn on") { watchTurnOn() })
    }
    actions.add(ui.button("Back") { showHome() })
    screen(AppWords.watchSetting.heading, views, actions, back = { showHome() }, words = AppWords.watchSetting)
}

/** Turning on: the notification permission first (OA-83), then the key, the file, the settings change and the service. */
private fun Controller.watchTurnOn() {
    val granted = act.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
    if (WatchPermission.mustAsk(Build.VERSION.SDK_INT, granted)) {
        @Suppress("DEPRECATION")
        act.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), Controller.REQUEST_NOTIFY)
        return
    }
    watchTurnOnGranted()
}

/** The answer of the notification permission's dialog: refused leaves the setting off, with words (OA-83). */
internal fun Controller.watchPermissionResult(results: IntArray) {
    if (results.isNotEmpty() && results[0] == PackageManager.PERMISSION_GRANTED) watchTurnOnGranted() else showWatch(WatchPermission.refused)
}

private fun Controller.watchTurnOnGranted() {
    val n = node ?: return begin()
    waiting(AppWords.working)
    async({ makeWatchReady(n) }) { r ->
        val e = r.exceptionOrNull()
        if (e != null) return@async showWatch(watchRefusal(e))
        val queued = r.getOrNull()
        WatchService.start(act)
        when {
            queued == null -> showWatch(null)
            queued.tightening -> centre(AppWords.editQueuedTightening, listOf(ui.primary("Done") { showWatch(null) }))
            else -> centre(AppWords.editQueuedLoosening, listOf(ui.primary("Build the request for the Key app") { showSpend() }, ui.button("Later") { showWatch(null) }))
        }
    }
}

/**
 * The key, the file, the settings change and the marker, in that order, so that a failure leaves the setting off.
 * Returns what was queued, or null when the settings already name this key.
 */
private fun Controller.makeWatchReady(n: OwnerNode): SettingsQueued? {
    var secret = if (watchStore.hasKey()) watchStore.loadSecret() else null
    var key: ByteArray? = secret?.let { OwnerWatch.makeKey(it) }
    if (secret == null || key == null) {
        val drawn = WatchSecret.draw(SecureRandom()) { OwnerWatch.makeKey(it) } ?: throw OwnerException("watch-key", "no watch key could be made", 4)
        secret = drawn.first
        key = drawn.second
        watchStore.saveSecret(secret)
    }
    try {
        var queued: SettingsQueued? = null
        val named = n.watchNamed()
        if (named == null || !named.contentEquals(key)) {
            val p = n.watchSettingsPreview(true, key)
            if (!p.nothingChanged) queued = n.queueSettings(p.settings)
        }
        watchStore.saveContents(n.watchExport(secret))
        watchStore.setEnabled(true)
        return queued
    } finally {
        secret.fill(0)
    }
}

/** Turning off: the service stops, the files go, and when the settings name the key its removal is queued (a tightening). */
private fun Controller.watchTurnOff() {
    val n = node ?: return begin()
    waiting(AppWords.working)
    async({
        WatchService.stop(act)
        watchStore.remove()
        if (n.watchNamed() != null) {
            val p = n.watchSettingsPreview(false, ByteArray(0))
            if (!p.nothingChanged) n.queueSettings(p.settings) else null
        } else {
            null
        }
    }) { r ->
        val e = r.exceptionOrNull()
        if (e != null) return@async showWatch(watchRefusal(e))
        if (r.getOrNull() != null) {
            centre(AppWords.editQueuedTightening, listOf(ui.primary("Done") { showWatch(null) }, ui.button("Waiting to be delivered") { showOutbox() }))
        } else {
            showWatch(null)
        }
    }
}

private fun Controller.watchRefusal(e: Throwable): SayWords = when (e) {
    is java.io.IOException -> AppWords.fileNotSaved(e.message)
    is java.security.GeneralSecurityException -> AppWords.storageUnavailable(e.message)
    else -> refusal(e)
}
