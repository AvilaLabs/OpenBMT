package org.openbmt.owner.app

import org.openbmt.app.SayQuoted
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerException
import org.openbmt.owner.Words

/** The owner binding's words, as the shared views and checks take them. */
fun Words.say(): SayWords =
    SayWords(tone, who, heading, what, next, code, at, quoted?.let { SayQuoted(it.from, it.text) }, detail)

/** The words of a refusal of the library, or the app's own words for an error it made itself. */
fun OwnerException.sayWords(): SayWords = words?.say() ?: AppWords.fault(this)
