package org.openbmt.app

import android.content.Context
import android.net.Uri

/** [PickedUri.accept] for the address the picker returned, in the app of [context]. */
fun PickedUri.accept(uri: Uri, context: Context): Boolean = accept(uri.scheme, uri.authority, context.packageName)
