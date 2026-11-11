package org.openbmt.owner.app

import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.widget.EditText
import android.widget.RadioGroup
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.SettingsHeld

// Changing the shop's settings (OA-32 to OA-35): from the settings view, "Change settings" opens a menu of six
// screens, each with the fields of one group, then "Review" shows every change. The node judges everything
// (AS-74 to AS-77): the app turns what was typed into an edit, asks the node what it would do, shows the answer,
// and only on "Queue the change" calls `queueSettings` with the bytes the node gave. The approval that follows is
// the one of `Approval.kt`; this file signs nothing and builds no settings bytes.

/** The node and the settings it holds, or the menu again with the words of why not. */
private inline fun Controller.withHeld(back: (SayWords?) -> Unit, block: (OwnerNode, SettingsHeld) -> Unit) {
    val n = node
    if (n == null) {
        begin()
        return
    }
    val held = try {
        n.settings()
    } catch (e: OwnerException) {
        back(e.sayWords())
        return
    } catch (e: LinkageError) {
        back(AppWords.libraryMissing(e.message))
        return
    }
    if (held == null) {
        back(AppWords.noSettingsYet)
        return
    }
    block(n, held)
}

/** The first screen of the editor: one button for each group, then Review. [message] is what the last step said. */
internal fun Controller.showChangeSettings(message: SayWords? = null) {
    withHeld({ w -> page(listOf(ui.text("Change settings", 22f, bold = true), ui.words(w ?: AppWords.noSettingsYet), ui.button("Back") { showSettings() })) }) { _, held ->
        val views = ArrayList<View>()
        views.add(ui.text("Change settings", 22f, bold = true))
        views.add(ui.text("Settings version ${held.version}", 14f))
        views.add(ui.words(AppWords.editIntro))
        message?.let { views.add(ui.words(it)) }
        for (g in SettingsEditModel.groups) {
            views.add(ui.button(g.words.heading) { showEditGroup(g) })
        }
        views.add(ui.button(AppWords.editGroupShares.heading) { showEditShares() })
        views.add(ui.button(AppWords.editGroupLightning.heading) { showEditLightning() })
        views.add(ui.button("Review") { showEditReview() })
        views.add(
            ui.button("Start over") {
                settingsDraft = SettingsDraft()
                showChangeSettings()
            },
        )
        views.add(ui.button("Back") { showSettings() })
        page(views)
    }
}

/** What one field of a group screen holds on the screen. */
private class FieldInput(val field: EditField, val text: EditText?, val choice: RadioGroup?)

/** The screen of the fields of one group: each with what it does and what it is now. Done keeps what was typed. */
private fun Controller.showEditGroup(group: EditGroup) {
    withHeld({ w -> showChangeSettings(w) }) { _, held ->
        val msg = Slot(act)
        val views = ArrayList<View>()
        views.add(ui.text(group.words.heading, 22f, bold = true))
        views.add(ui.words(group.words))
        val inputs = ArrayList<FieldInput>()
        for (f in group.fields) {
            views.add(label(f.label))
            views.add(ui.text(f.words.what, 15f))
            views.add(ui.text("Now: ${SettingsEditModel.now(f, held)}", 15f, bold = true))
            val typed = settingsDraft.typed[f.id]
            if (f.kind == FieldKind.Switch) {
                val on = (typed ?: f.read(held).toString()) == "1"
                val options = if (f.id == "customer_wifi") listOf("Yes or unsure", "No") else listOf("On", "Off")
                val choice = ui.choice(options, if (on) 0 else 1)
                views.add(choice)
                inputs.add(FieldInput(f, null, choice))
            } else {
                val field = ui.field(f.words.next)
                field.setText(typed ?: SettingsEditModel.input(f, held))
                views.add(field)
                views.add(ui.text(f.words.next, 13f))
                inputs.add(FieldInput(f, field, null))
            }
        }
        views.add(
            ui.button("Done") {
                val keep = LinkedHashMap<String, String?>()
                for (i in inputs) {
                    val f = i.field
                    val text = if (i.choice != null) {
                        when (ui.chosen(i.choice)) {
                            0 -> "1"
                            1 -> "0"
                            else -> null
                        }
                    } else {
                        i.text?.text?.toString()
                    }
                    if (text == null || SettingsEditModel.parse(f.kind, text) == null) {
                        msg.show(ui.words(AppWords.editValueNotRead(f.kind, f.label)))
                        return@button
                    }
                    val same = if (f.kind == FieldKind.Switch) text == f.read(held).toString() else text == SettingsEditModel.input(f, held)
                    keep[f.id] = if (same) null else text
                }
                for ((id, text) in keep) {
                    if (text == null) settingsDraft.typed.remove(id) else settingsDraft.typed[id] = text
                }
                showChangeSettings()
            },
        )
        views.add(msg)
        views.add(ui.button("Back") { showChangeSettings() })
        page(views)
    }
}

/** The shares of the counters, each with what it is now, and the total and what remains as they are typed. */
private fun Controller.showEditShares() {
    withHeld({ w -> showChangeSettings(w) }) { _, held ->
        val msg = Slot(act)
        val views = ArrayList<View>()
        views.add(ui.text(AppWords.editGroupShares.heading, 22f, bold = true))
        views.add(ui.words(AppWords.editGroupShares))
        if (held.counters.isEmpty()) {
            views.add(ui.words(AppWords.editNoCounters))
            views.add(ui.button("Back") { showChangeSettings() })
            page(views)
            return@withHeld
        }
        val total = ui.text("", 15f, bold = true)
        val inputs = ArrayList<Pair<ByteArray, EditText>>()
        fun refresh() {
            total.text = SettingsEditModel.sharesLine(inputs.map { it.second.text.toString() })
        }
        for (s in held.counters) {
            views.add(label(SettingsEditModel.shareNow(s)))
            val field = ui.field("Percent")
            field.setText(settingsDraft.shares[SettingsEditModel.counterKey(s.counter)] ?: SettingsEditModel.percentInput(s.bps))
            field.addTextChangedListener(
                object : TextWatcher {
                    override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}

                    override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}

                    override fun afterTextChanged(s: Editable?) = refresh()
                },
            )
            views.add(field)
            inputs.add(s.counter to field)
        }
        views.add(total)
        refresh()
        views.add(
            ui.button("Done") {
                val keep = LinkedHashMap<String, String?>()
                for ((counter, field) in inputs) {
                    val text = field.text.toString()
                    if (SettingsEditModel.percentBps(text) == null) {
                        msg.show(ui.words(AppWords.editSharesNotPercent))
                        return@button
                    }
                    val now = held.counters.first { it.counter.contentEquals(counter) }.bps
                    keep[SettingsEditModel.counterKey(counter)] = if (text == SettingsEditModel.percentInput(now)) null else text
                }
                for ((id, text) in keep) {
                    if (text == null) settingsDraft.shares.remove(id) else settingsDraft.shares[id] = text
                }
                showChangeSettings()
            },
        )
        views.add(msg)
        views.add(ui.button("Back") { showChangeSettings() })
        page(views)
    }
}

/** The Lightning node: its key in full as the settings hold it, the words about the Key app and the counters, and the ways to name or remove it. */
private fun Controller.showEditLightning(prefill: String? = null, message: SayWords? = null) {
    withHeld({ w -> showChangeSettings(w) }) { _, held ->
        val msg = Slot(act)
        val views = ArrayList<View>()
        views.add(ui.text(AppWords.editGroupLightning.heading, 22f, bold = true))
        views.add(ui.words(AppWords.editGroupLightning))
        views.add(ui.words(AppWords.editLightningNote))
        message?.let { views.add(ui.words(it)) }
        views.add(label("Now"))
        views.add(ui.text(SettingsEditModel.nodeNow(held), 13f, mono = true))
        val field = ui.field("Node key or address", lines = 3)
        field.setText(prefill ?: if (settingsDraft.node == NodeChoice.Set) settingsDraft.nodeText else "")
        views.add(field)
        views.add(ui.button("Scan a code") { scanNode() })
        views.add(
            ui.button("Use this") {
                when (val t = SettingsEditModel.nodeText(field.text.toString())) {
                    is Form.Refused -> msg.show(ui.words(t.words))
                    is Form.Ok -> {
                        settingsDraft.node = NodeChoice.Set
                        settingsDraft.nodeText = t.value
                        showChangeSettings()
                    }
                }
            },
        )
        if (held.lightningNode != null) {
            views.add(
                ui.button("Remove the node") {
                    settingsDraft.node = NodeChoice.Remove
                    settingsDraft.nodeText = ""
                    showChangeSettings()
                },
            )
        }
        views.add(
            ui.button("Keep it as it is") {
                settingsDraft.node = NodeChoice.Keep
                settingsDraft.nodeText = ""
                showChangeSettings()
            },
        )
        views.add(msg)
        views.add(ui.button("Back") { showChangeSettings() })
        page(views)
    }
}

/**
 * The node's code, read by the camera, which is asked for only when the button is pressed. A plain code's text goes
 * into the field; the library judges it when the change is reviewed (AS-76).
 */
private fun Controller.scanNode() {
    fun give(r: SourceResult) = when (r) {
        is SourceResult.Bytes -> showEditLightning(prefill = String(r.bytes, Charsets.UTF_8))
        is SourceResult.Failed -> showEditLightning(message = r.words)
    }
    scan.begin(
        show = { v ->
            tick = null
            act.show(v)
        },
        onBytes = { give(SourceResult.Bytes(it)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.counterScanCancelled)) },
    )
}

/**
 * Every change, old to new, those that need the Key app and those that take effect when delivered, then "Queue the
 * change" or "Back". The node makes the preview and stores nothing; only the queue button stores (AS-77).
 */
internal fun Controller.showEditReview() {
    withHeld({ w -> showChangeSettings(w) }) { n, held ->
        val args = when (val a = SettingsEditModel.args(settingsDraft, held)) {
            is Form.Refused -> {
                showChangeSettings(a.words)
                return@withHeld
            }
            is Form.Ok -> a.value
        }
        val p = try {
            n.previewSettings(args)
        } catch (e: OwnerException) {
            if (e.rule == "AS-75") showEditPrice(e.sayWords()) else showChangeSettings(e.sayWords())
            return@withHeld
        } catch (e: LinkageError) {
            showChangeSettings(AppWords.libraryMissing(e.message))
            return@withHeld
        }
        val msg = Slot(act)
        val views = ArrayList<View>()
        views.add(ui.text("Review the change", 22f, bold = true))
        views.add(ui.words(SettingsEditModel.reviewWords(p)))
        if (!p.nothingChanged) {
            val keyApp = SettingsEditModel.needKeyApp(p)
            if (keyApp.isNotEmpty()) {
                views.add(label("Needs the Key app"))
                for (line in keyApp) views.add(ui.text(line, 15f))
            }
            val delivered = SettingsEditModel.whenDelivered(p)
            if (delivered.isNotEmpty()) {
                views.add(label("Takes effect when delivered"))
                for (line in delivered) views.add(ui.text(line, 15f))
            }
            SettingsEditModel.reviewShares(p)?.let { views.add(ui.text(it, 15f)) }
            views.add(
                ui.button("Queue the change") {
                    val q = attempt(msg) { n.queueSettings(p.settings) } ?: return@button
                    settingsDraft = SettingsDraft()
                    if (q.tightening) {
                        page(listOf(ui.words(AppWords.editQueuedTightening), ui.button("Waiting to be delivered") { showOutbox() }, ui.button("Done") { showHome() }))
                    } else {
                        page(listOf(ui.words(AppWords.editQueuedLoosening), ui.button("Build the request for the Key app") { showSpend() }, ui.button("Later") { showHome() }))
                    }
                },
            )
        }
        views.add(msg)
        views.add(
            ui.button("Back") {
                settingsDraft.price = ""
                showChangeSettings()
            },
        )
        page(views)
    }
}

/**
 * The library said a price is needed, or that the one typed is not one: its words, an empty field for today's price
 * of one bitcoin in whole dollars (never filled in), and the review again. The price is forgotten on the way in and
 * on the way out, so it is typed afresh each time (OA-34).
 */
private fun Controller.showEditPrice(words: SayWords) {
    settingsDraft.price = ""
    val msg = Slot(act)
    val field = numberField(act, "Dollars for one bitcoin")
    page(
        listOf(
            ui.text("Today's price", 22f, bold = true),
            ui.words(words),
            field,
            ui.button("Review again") {
                when (val r = CounterModel.price(field.text.toString())) {
                    is Form.Refused -> msg.show(ui.words(r.words))
                    is Form.Ok -> {
                        settingsDraft.price = field.text.toString()
                        showEditReview()
                    }
                }
            },
            msg,
            ui.button("Back") { showChangeSettings() },
        ),
    )
}
