package org.openbmt.checkout

import android.app.Activity
import android.app.AlertDialog
import android.graphics.Color
import android.text.InputType
import android.text.InputFilter
import android.text.method.DigitsKeyListener
import android.text.method.PasswordTransformationMethod
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/** Programmatic, non-persisting UI for fresh manager enrollment and staff sign-in. */
internal class CheckoutAuthorityUi(private val activity: Activity) {
    interface Actions {
        fun refresh()
        fun prepareManagerEnrollment()
        fun confirmRecoveryCode(value: CharArray)
        fun enrollManager(name: String, pin: CharArray)
        fun signIn(actorId: String, pin: CharArray)
        fun beginManagerRecovery()
        fun prepareManagerRecovery(currentCode: CharArray, newPin: CharArray)
        fun confirmNextRecoveryCode(value: CharArray)
    }

    interface StaffActions {
        fun reload()
        fun create(name: String, role: CheckoutAuthorityRole, pin: CharArray)
        fun rename(actorId: String, name: String)
        fun changeRole(actorId: String, role: CheckoutAuthorityRole)
        fun changePin(actorId: String, pin: CharArray)
        fun disable(actorId: String)
        fun close()
    }

    private var selectedActor: CheckoutStaffSummary? = null

    fun render(state: CheckoutAccessState, actions: Actions) {
        selectedActor?.let { actor ->
            if (state !is CheckoutAccessState.SignInRequired) selectedActor = null
            else if (state.staff.none { it.actorId == actor.actorId && it.enabled }) selectedActor = null
        }
        val column = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(30), dp(24), dp(24))
            filterTouchesWhenObscured = true
        }
        column.addView(label("OpenBMT · TEST ONLY · REGTEST", 12f, true), margins(bottom = 8))
        val title = when (state) {
            CheckoutAccessState.Loading -> "Checking access"
            CheckoutAccessState.EnrollmentRequired -> "Set up manager access"
            is CheckoutAccessState.RecoveryCodeConfirmation -> "Save your recovery code"
            is CheckoutAccessState.ManagerEnrollmentRequired -> "Create manager PIN"
            is CheckoutAccessState.SignInRequired -> if (selectedActor == null) "Sign in" else "Enter PIN"
            is CheckoutAccessState.SignedIn -> "Access ready"
            is CheckoutAccessState.ManagerRecoveryRequired -> "Recover manager access"
            is CheckoutAccessState.ManagerRecoveryCodeAcknowledgement -> "Save replacement recovery code"
            is CheckoutAccessState.Unavailable -> "Checkout access unavailable"
        }
        column.addView(label(title, 24f, true), margins(bottom = 12))
        when (state) {
            CheckoutAccessState.Loading -> column.addView(label("Checkout and the Regtest counter stay locked until local authority is checked.", 15f), margins())
            CheckoutAccessState.EnrollmentRequired -> {
                column.addView(label("Create the first manager on this fresh test installation. Existing checkout records without their installation authority cannot be adopted automatically.", 15f), margins(bottom = 10))
                addButton(column, "Begin manager setup") { actions.prepareManagerEnrollment() }
            }
            is CheckoutAccessState.RecoveryCodeConfirmation -> {
                column.addView(label("Write this recovery code down and store it offline. It is shown once for this setup attempt. Do not share it.", 15f), margins(bottom = 12))
                val code = label(state.recoveryCode, 20f, true, mono = true).apply {
                    setSaveEnabled(false)
                    setTextIsSelectable(false)
                    contentDescription = "One-time recovery code"
                }
                column.addView(code, margins(bottom = 16))
                column.addView(label("Type the complete code to confirm you saved it.", 14f), margins(bottom = 6))
                val confirmation = recoveryCodeField("Recovery code")
                column.addView(confirmation, margins(bottom = 8))
                state.message?.let { column.addView(label(it, 14f), margins(bottom = 8)) }
                addButton(column, "Confirm recovery code") {
                    val chars = copyAndClear(confirmation)
                    actions.confirmRecoveryCode(chars)
                }
            }
            is CheckoutAccessState.ManagerEnrollmentRequired -> {
                column.addView(label("Set a 6–12 digit numeric PIN for the manager. Sign-in is required before checkout or the counter can open.", 15f), margins(bottom = 10))
                val name = textField("Manager name", numeric = false)
                val pin = textField("Manager PIN", numeric = true)
                column.addView(name, margins(bottom = 8))
                column.addView(pin, margins(bottom = 8))
                state.message?.let { column.addView(label(it, 14f), margins(bottom = 8)) }
                addButton(column, "Create manager") {
                    val displayName = name.text.toString().trim().take(80)
                    name.text.clear()
                    val secret = copyAndClear(pin)
                    actions.enrollManager(displayName, secret)
                }
            }
            is CheckoutAccessState.SignInRequired -> {
                val actor = selectedActor
                if (actor == null) {
                    column.addView(label("Choose the staff member using this register.", 15f), margins(bottom = 8))
                    state.staff.filter { it.enabled }.forEach { staff ->
                        addButton(column, "${staff.displayName} · ${staff.role.name.lowercase().replaceFirstChar(Char::uppercase)}") {
                            selectedActor = staff
                            render(state, actions)
                        }
                    }
                } else {
                    column.addView(label("${actor.displayName} · ${actor.role.name.lowercase().replaceFirstChar(Char::uppercase)}", 15f), margins(bottom = 8))
                    val pin = textField("Numeric PIN", numeric = true)
                    column.addView(pin, margins(bottom = 8))
                    addButton(column, "Sign in") { actions.signIn(actor.actorId, copyAndClear(pin)) }
                    addButton(column, "Choose another staff member") {
                        selectedActor = null
                        render(state, actions)
                    }
                }
                state.message?.let { column.addView(label(it, 14f), margins(top = 8)) }
                addButton(column, "Recover manager access") { actions.beginManagerRecovery() }
            }
            is CheckoutAccessState.SignedIn -> column.addView(label("Signed in as ${state.actor.displayNameAtEvent}.", 15f), margins())
            is CheckoutAccessState.ManagerRecoveryRequired -> {
                column.addView(label("Enter the current 64-character recovery code and choose a new 6–12 digit manager PIN. Recovery changes manager access only; it does not reset checkout records.", 15f), margins(bottom = 10))
                val currentCode = recoveryCodeField("Current recovery code")
                val newPin = textField("New manager PIN", numeric = true)
                column.addView(currentCode, margins(bottom = 8))
                column.addView(newPin, margins(bottom = 8))
                state.message?.let { column.addView(label(it, 14f), margins(bottom = 8)) }
                addButton(column, "Prepare recovery") {
                    val codeChars = copyAndClear(currentCode)
                    val pinChars = copyAndClear(newPin)
                    actions.prepareManagerRecovery(codeChars, pinChars)
                }
                addButton(column, "Back to sign in") { actions.refresh() }
            }
            is CheckoutAccessState.ManagerRecoveryCodeAcknowledgement -> {
                column.addView(label("Write this replacement recovery code down and store it offline. It replaces the previous code when recovery commits.", 15f), margins(bottom = 12))
                column.addView(label(state.nextRecoveryCode, 20f, true, mono = true).apply {
                    setSaveEnabled(false)
                    setTextIsSelectable(false)
                    contentDescription = "Replacement one-time recovery code"
                }, margins(bottom = 14))
                column.addView(label("Type the full replacement code to commit manager recovery.", 14f), margins(bottom = 6))
                val confirmation = recoveryCodeField("Replacement recovery code")
                column.addView(confirmation, margins(bottom = 8))
                state.message?.let { column.addView(label(it, 14f), margins(bottom = 8)) }
                if (state.commitUncertain) {
                    column.addView(label("Do not retry recovery in this app session. Keep the replacement code and verify access through the supported recovery procedure.", 14f, true), margins())
                } else {
                    addButton(column, "Confirm code and recover") {
                        actions.confirmNextRecoveryCode(copyAndClear(confirmation))
                    }
                }
            }
            is CheckoutAccessState.Unavailable -> {
                column.addView(label("Local authority or installation storage could not be verified. No checkout data was reset and the counter will not start. Restore the original app data or use the supported recovery process.", 15f), margins(bottom = 10))
                if (state.managerRecoveryAvailable) addButton(column, "Recover manager access") { actions.beginManagerRecovery() }
                addButton(column, "Retry access check") { actions.refresh() }
            }
        }
        val scroll = ScrollView(activity).apply {
            isFillViewport = true
            addView(column, ViewGroup.LayoutParams(-1, -2))
        }
        activity.setContentView(scroll)
    }

    fun renderStaffAdministration(
        snapshot: CheckoutStaffAdministrationSnapshot?,
        message: String?,
        actions: StaffActions,
    ) {
        val column = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(24), dp(24), dp(24))
            filterTouchesWhenObscured = true
        }
        column.addView(label("STAFF MANAGEMENT · TEST ONLY", 12f, true), margins(bottom = 8))
        column.addView(label("Manage staff access", 24f, true), margins(bottom = 10))
        column.addView(label("Each staff write requires current manager authorization and records an audit intent before changing access. Opening this screen checks any pending intent against the durable authority receipt and saved staff state. If neither proves the outcome, staff changes stay blocked. Staff updates require sign-in again.", 14f), margins(bottom = 12))
        column.addView(label("The first manager is the recovery manager. The current authority model does not support changing that designation.", 13f), margins(bottom = 12))
        message?.let { column.addView(label(it, 14f, true), margins(bottom = 10)) }

        var newRole = CheckoutAuthorityRole.CASHIER
        val createName = textField("Staff name", numeric = false)
        val createPin = textField("Initial 6–12 digit PIN", numeric = true)
        val roleButton = Button(activity).apply {
            isAllCaps = false
            minHeight = dp(48)
            text = "New role: Cashier"
            setOnClickListener {
                newRole = if (newRole == CheckoutAuthorityRole.CASHIER) CheckoutAuthorityRole.MANAGER else CheckoutAuthorityRole.CASHIER
                text = "New role: ${newRole.name.lowercase().replaceFirstChar(Char::uppercase)}"
            }
        }
        column.addView(label("Create staff member", 18f, true), margins(top = 8, bottom = 6))
        column.addView(createName, margins(bottom = 6))
        column.addView(roleButton, margins(bottom = 6))
        column.addView(createPin, margins(bottom = 6))
        addButton(column, "Create staff member") {
            val name = createName.text.toString().trim().take(80)
            createName.text.clear()
            val pin = copyAndClear(createPin)
            actions.create(name, newRole, pin)
        }

        val staff = snapshot?.staff.orEmpty()
        column.addView(label("Staff directory", 18f, true), margins(top = 18, bottom = 8))
        if (snapshot == null) column.addView(label("Loading staff directory…", 14f), margins())
        else if (staff.isEmpty()) column.addView(label("No staff records are available.", 14f), margins())
        staff.forEach { person ->
            column.addView(label("${person.displayName} · ${person.role.name.lowercase().replaceFirstChar(Char::uppercase)} · ${if (person.enabled) "enabled" else "disabled"}", 16f, true), margins(top = 10, bottom = 4))
            column.addView(label(person.actorId, 11f, mono = true), margins(bottom = 6))
            val rename = textField("New staff name", numeric = false).apply { setText(person.displayName) }
            column.addView(rename, margins(bottom = 5))
            addButton(column, "Save name") {
                val name = rename.text.toString().trim().take(80)
                rename.text.clear()
                actions.rename(person.actorId, name)
            }
            val nextRole = if (person.role == CheckoutAuthorityRole.MANAGER) CheckoutAuthorityRole.CASHIER else CheckoutAuthorityRole.MANAGER
            addButton(column, "Change role to ${nextRole.name.lowercase().replaceFirstChar(Char::uppercase)}") {
                actions.changeRole(person.actorId, nextRole)
            }
            val newPin = textField("New 6–12 digit PIN", numeric = true)
            column.addView(newPin, margins(top = 5, bottom = 4))
            addButton(column, "Change PIN") { actions.changePin(person.actorId, copyAndClear(newPin)) }
            if (person.enabled) addButton(column, "Disable staff member") {
                AlertDialog.Builder(activity)
                    .setTitle("Disable ${person.displayName}?")
                    .setMessage("They will no longer be able to sign in. This change requires manager reauthentication.")
                    .setNegativeButton("Cancel", null)
                    .setPositiveButton("Disable") { _, _ -> actions.disable(person.actorId) }
                    .show()
            }
        }
        column.addView(button("Refresh staff directory") { actions.reload() }, margins(top = 18))
        column.addView(button("Back") { actions.close() }, margins(top = 8))
        val scroll = ScrollView(activity).apply {
            isFillViewport = true
            addView(column, ViewGroup.LayoutParams(-1, -2))
        }
        activity.setContentView(scroll)
    }

    private fun textField(hintText: String, numeric: Boolean): EditText = EditText(activity).apply {
        hint = hintText
        isSingleLine = true
        setSaveEnabled(false)
        isLongClickable = false
        inputType = if (numeric) InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
            else InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        if (numeric) {
            keyListener = DigitsKeyListener.getInstance("0123456789")
            transformationMethod = PasswordTransformationMethod.getInstance()
            filters = arrayOf(InputFilter.LengthFilter(12))
        }
        importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS
        imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI
        filterTouchesWhenObscured = true
    }

    private fun recoveryCodeField(hintText: String): EditText = EditText(activity).apply {
        hint = hintText
        isSingleLine = true
        setSaveEnabled(false)
        isLongClickable = false
        keyListener = DigitsKeyListener.getInstance("0123456789abcdef")
        filters = arrayOf(InputFilter.LengthFilter(64))
        inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        transformationMethod = PasswordTransformationMethod.getInstance()
        importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS
        imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI
        filterTouchesWhenObscured = true
    }

    private fun copyAndClear(field: EditText): CharArray {
        val value = field.text
        val result = CharArray(value.length) { value[it] }
        value.clear()
        field.clearFocus()
        return result
    }

    private fun addButton(parent: LinearLayout, title: String, action: () -> Unit) {
        parent.addView(Button(activity).apply {
            text = title
            isAllCaps = false
            minHeight = dp(52)
            filterTouchesWhenObscured = true
            setOnClickListener { action() }
        }, margins(top = 8))
    }

    private fun button(title: String, action: () -> Unit) = Button(activity).apply {
        text = title
        isAllCaps = false
        minHeight = dp(52)
        filterTouchesWhenObscured = true
        setOnClickListener { action() }
    }

    private fun label(value: String, size: Float, bold: Boolean = false, mono: Boolean = false) = TextView(activity).apply {
        text = value
        textSize = size
        setTextColor(Color.rgb(24, 34, 43))
        gravity = Gravity.START
        if (bold) setTypeface(typeface, android.graphics.Typeface.BOLD)
        if (mono) typeface = android.graphics.Typeface.MONOSPACE
        filterTouchesWhenObscured = true
    }

    private fun margins(top: Int = 0, bottom: Int = 0) = LinearLayout.LayoutParams(-1, -2).also {
        it.topMargin = dp(top)
        it.bottomMargin = dp(bottom)
    }

    private fun dp(value: Int) = (value * activity.resources.displayMetrics.density).toInt()
}
