package org.openbmt.counter.app

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.File
import java.io.FileNotFoundException

/**
 * CR-214: hands the share sheet the one image of a request's code, by one-time access. It is not exported; the
 * counter grants the chosen app read access to this one address when the person taps "Share the code", and the
 * file is deleted when the page is left. It serves only [NAME] in the app's own cache folder, read only.
 */
class ShareImageProvider : ContentProvider() {
    override fun onCreate(): Boolean = true

    private fun file(uri: Uri): File? {
        val ctx = context ?: return null
        if (uri.pathSegments != listOf(FOLDER, NAME)) return null
        return File(File(ctx.cacheDir, FOLDER), NAME).takeIf { it.isFile }
    }

    override fun getType(uri: Uri): String? = if (uri.pathSegments == listOf(FOLDER, NAME)) "image/png" else null

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor? {
        val f = file(uri) ?: return null
        val cursor = MatrixCursor(arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), 1)
        cursor.addRow(arrayOf<Any>(NAME, f.length()))
        return cursor
    }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        if (mode != "r") throw FileNotFoundException("read only")
        val f = file(uri) ?: throw FileNotFoundException("no image")
        return ParcelFileDescriptor.open(f, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int = 0

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0

    companion object {
        const val AUTHORITY = "org.openbmt.counter.app.share"
        const val FOLDER = "share"
        const val NAME = "request.png"
    }
}
