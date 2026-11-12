package org.openbmt.key.app

/**
 * The release build of the Key app: no development copy (SR-91). The dev
 * variant, `src/variant/dev/DevCopy.kt`, is compiled instead of this file
 * only by `build.sh --dev`; nothing at run time can switch this on.
 */
object DevCopy {
    const val ON = false
    const val BAND = ""
}
