package org.openbmt.key.app

import org.openbmt.app.SayQuoted
import org.openbmt.app.SayWords
import org.openbmt.key.Words

/** The Key binding's words, as the shared views and checks take them. */
fun Words.say(): SayWords =
    SayWords(tone, who, heading, what, next, code, at, quoted?.let { SayQuoted(it.from, it.text) }, detail)
