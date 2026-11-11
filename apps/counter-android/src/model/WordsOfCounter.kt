package org.openbmt.counter.app

import org.openbmt.app.SayQuoted
import org.openbmt.app.SayWords
import org.openbmt.counter.CounterException
import org.openbmt.counter.Words

/** The counter binding's words, as the shared views and checks take them. */
fun Words.say(): SayWords =
    SayWords(tone, who, heading, what, next, code, at, quoted?.let { SayQuoted(it.from, it.text) }, detail)

/** The words of a refusal of the library, or the app's own words for an error it made itself. */
fun CounterException.sayWords(): SayWords = words?.say() ?: AppWords.fault(this)
