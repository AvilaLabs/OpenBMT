package org.openbmt.app

/**
 * Which file the system's picker may hand over. The apps read or write only a document that a documents provider
 * gave: a `content` address whose authority is not one of the app's own. A `file` address, no scheme, or an address
 * of the app's own provider (its sealed state is in its own folder) is refused, so that an app that answers the
 * picker cannot make the app overwrite or read its own files.
 */
object PickedUri {
    /** True when [scheme] and [authority] (as `Uri.getScheme` and `Uri.getAuthority` give them) may be read or written. */
    fun accept(scheme: String?, authority: String?, ownPackage: String): Boolean {
        if (scheme == null || !scheme.equals("content", ignoreCase = true)) return false
        if (authority == null || ownPackage.isEmpty()) return false
        // An authority may carry the user it is for before an @ (content://10@provider/...).
        val host = authority.substringAfterLast('@').lowercase()
        if (host.isEmpty()) return false
        return !host.startsWith(ownPackage.lowercase())
    }
}
