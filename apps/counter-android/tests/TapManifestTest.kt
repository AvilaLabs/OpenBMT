package org.openbmt.counter.app

import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Document
import org.w3c.dom.Element

/**
 * TP-10, TP-21, TP-22, TP-23, TP-50: what the manifest and the tag's service file say, read as the source files,
 * and the parts of the tag service that need no Android class. Arguments: the app folder and the repository.
 */
object TapManifestTest {
    private const val ANDROID = "http://schemas.android.com/apk/res/android"
    private var checks = 0

    private fun ok(cond: Boolean, what: String) {
        checks++
        if (!cond) throw AssertionError(what)
    }

    private fun parse(file: File): Document =
        DocumentBuilderFactory.newInstance().apply { isNamespaceAware = true }.newDocumentBuilder().parse(file)

    private fun Element.attr(name: String): String = getAttributeNS(ANDROID, name)

    private fun Document.all(tag: String): List<Element> =
        getElementsByTagName(tag).let { n -> (0 until n.length).map { n.item(it) as Element } }

    private fun Element.children(tag: String): List<Element> =
        getElementsByTagName(tag).let { n -> (0 until n.length).map { n.item(it) as Element } }

    @JvmStatic
    fun main(args: Array<String>) {
        val app = File(args[0])
        val repo = File(args[1])
        manifest(app)
        serviceFile(app, repo)
        source(app)
        model()
        println("tap manifest and service tests: $checks checks passed")
    }

    private fun manifest(app: File) {
        val text = File(app, "AndroidManifest.xml").readText()
        val doc = parse(File(app, "AndroidManifest.xml"))
        // TP-22, TP-10: one service, the tag's, bound by the system only, and no other.
        val services = doc.all("service")
        ok(services.size == 1, "one service")
        val service = services[0]
        ok(service.attr("name") == "org.openbmt.counter.app.TapService", "the tag service")
        ok(service.attr("permission") == "android.permission.BIND_NFC_SERVICE", "bound by BIND_NFC_SERVICE")
        ok(service.attr("exported") == "true", "exported, as Android's NFC service must reach it")
        val filters = service.children("intent-filter")
        ok(filters.size == 1, "one intent filter")
        val actions = filters[0].children("action")
        ok(actions.size == 1 && actions[0].attr("name") == "android.nfc.cardemulation.action.HOST_APDU_SERVICE", "the HOST_APDU_SERVICE action")
        ok(filters[0].children("category").isEmpty() && filters[0].children("data").isEmpty(), "no category or data in the filter")
        val meta = service.children("meta-data")
        ok(meta.size == 1, "one meta-data")
        ok(meta[0].attr("name") == "android.nfc.cardemulation.host_apdu_service" && meta[0].attr("resource") == "@xml/tap_aid", "the service file")
        ok(doc.all("receiver").isEmpty(), "no receiver")
        // Only the launcher activity and the tag service are exported.
        val exported = (doc.all("activity") + doc.all("service") + doc.all("provider") + doc.all("receiver"))
            .filter { it.attr("exported") == "true" }.map { it.attr("name") }.sorted()
        ok(exported == listOf("org.openbmt.counter.app.MainActivity", "org.openbmt.counter.app.TapService"), "exported: $exported")
        // TP-23: no discovery of tags, tag technologies or NDEF messages, anywhere, and no NFC intent at all.
        for (word in listOf("NDEF_DISCOVERED", "TECH_DISCOVERED", "TAG_DISCOVERED", "nfc.action", "tech-list", "nfc_tech_filter")) {
            ok(!text.contains(word), "the manifest has no $word")
        }
        val intents = doc.all("action").map { it.attr("name") }.sorted()
        ok(intents == listOf("android.intent.action.MAIN", "android.nfc.cardemulation.action.HOST_APDU_SERVICE"), "actions: $intents")
        // The permission and the features.
        val permissions = doc.all("uses-permission").map { it.attr("name") }.sorted()
        ok(permissions == listOf("android.permission.CAMERA", "android.permission.INTERNET", "android.permission.NFC"), "permissions: $permissions")
        val features = doc.all("uses-feature").associate { it.attr("name") to it.attr("required") }
        ok(features["android.hardware.nfc.hce"] == "false", "card emulation is not required")
    }

    private fun serviceFile(app: File, repo: File) {
        val doc = parse(File(app, "res/xml/tap_aid.xml"))
        val root = doc.documentElement
        ok(root.tagName == "host-apdu-service", "a host-apdu-service")
        // TP-21: both attributes are in the android-36 platform (HostApduService); no other attribute is used.
        ok(root.attr("requireDeviceUnlock") == "true", "device unlock required")
        ok(root.attr("requireDeviceScreenOn") == "true", "screen on required")
        val names = (0 until root.attributes.length).map { root.attributes.item(it).nodeName }.filterNot { it.startsWith("xmlns") }.sorted()
        ok(names == listOf("android:description", "android:requireDeviceScreenOn", "android:requireDeviceUnlock"), "attributes: $names")
        // TP-10: one AID, in the category other, and the library answers to the same one.
        val groups = doc.all("aid-group")
        ok(groups.size == 1, "one aid-group")
        ok(groups[0].attr("category") == "other", "category other")
        ok(doc.all("aid-prefix-filter").isEmpty(), "no prefix filter")
        val filters = doc.all("aid-filter")
        ok(filters.size == 1, "one aid-filter")
        val aid = filters[0].attr("name")
        ok(aid == "D2760000850101", "the application ID")
        val tap = File(repo, "crates/openbmt-counter-ffi/src/tap.rs").readText()
        val inLibrary = Regex("const AID: \\[u8; 7\\] = \\[([^\\]]*)\\]").find(tap)!!.groupValues[1]
            .split(",").map { it.trim() }.filter { it.isNotEmpty() }.joinToString("") { it.removePrefix("0x") }
        ok(inLibrary.equals(aid, ignoreCase = true), "the library's AID is the file's: $inLibrary")
    }

    private fun source(app: File) {
        val service = File(app, "src/android/TapService.kt").readText()
        // TP-21: both checks on every command.
        ok(service.contains("isInteractive") && service.contains("isDeviceLocked"), "the screen and the lock are checked")
        // Nothing of a command or of the tag is logged.
        for (word in listOf("Log.", "println", "printStackTrace", "Timber")) ok(!service.contains(word), "no $word in the service")
        // TP-20: preferred in onResume, not preferred in onPause, both through the controller.
        val main = File(app, "src/android/MainActivity.kt").readText()
        ok(Regex("override fun onResume\\(\\) \\{[^}]*tapFront\\(true\\)").containsMatchIn(main), "onResume prefers the tag service")
        ok(Regex("override fun onPause\\(\\) \\{[^}]*tapFront\\(false\\)").containsMatchIn(main), "onPause stops")
        ok(service.contains("setPreferredService") && service.contains("unsetPreferredService"), "both calls are made")
        // TP-23: nothing reads a tag.
        for (word in listOf("enableReaderMode", "enableForegroundDispatch", "NfcAdapter.ACTION", "Ndef", "IsoDep")) {
            ok(!service.contains(word) && !File(app, "src/android/Controller.kt").readText().contains(word), "no $word")
        }
    }

    private fun model() {
        // TP-04: no tag until one is set; an empty one is none.
        TapTag.clear()
        ok(TapTag.current() == null, "empty at the start")
        TapTag.set(byteArrayOf(1, 2, 3))
        ok(TapTag.current()!!.toList() == listOf<Byte>(1, 2, 3), "the tag as set")
        TapTag.set(byteArrayOf(9))
        ok(TapTag.current()!!.toList() == listOf<Byte>(9), "the tag as changed (TP-05)")
        TapTag.set(byteArrayOf())
        ok(TapTag.current() == null, "an empty tag (a request that does not fit) is none")
        TapTag.set(byteArrayOf(1))
        TapTag.set(null)
        ok(TapTag.current() == null, "cleared")
        // TP-20: preferred only while the activity is in front and a tag is set.
        val calls = ArrayList<Boolean>()
        val p = TapPreference { calls.add(it) }
        p.resumed(true)
        ok(calls.isEmpty(), "in front with no sale: nothing asked")
        p.wanted(true)
        ok(calls == listOf(true), "a sale on the screen in front")
        p.wanted(true)
        ok(calls == listOf(true), "the same state asks nothing again")
        p.resumed(false)
        ok(calls == listOf(true, false), "the screen left")
        p.resumed(true)
        ok(calls == listOf(true, false, true), "back in front with the sale")
        p.wanted(false)
        ok(calls == listOf(true, false, true, false), "the sale ended")
    }
}
