package org.openbmt.app

/** The words of reading and showing a code that changes, for both apps; [who] is the one who does the next step in that app. */
object UrWords {
    /** Part of an animated code has been read; [received] of about [expected] parts. */
    fun reading(who: Int, received: Int, expected: Int) = SayWords(
        Tones.INFO, who, "Reading the animated code",
        "Reading the animated code: $received of about $expected parts.",
        "Keep the phone pointed at the code until it is done.",
        null, null, null, null,
    )

    /** The parts read did not fit together; reading went back to the start. */
    fun restarted(who: Int) = SayWords(
        Tones.STOP, who, "Code did not add up",
        "The parts read did not fit together, so reading starts again.",
        "Keep the phone pointed at the code. It reads from the first part again.",
        null, null, null, null,
    )

    /** Many parts of a different code in a row. */
    fun otherCode(who: Int) = SayWords(
        Tones.INFO, who, "Another code is in view",
        "The code in view is not part of the one being read, so it is skipped.",
        "Point the phone at the first code again, or cancel and start over.",
        null, null, null, null,
    )

    /** The code was read in full but is of a type this step does not take. */
    fun wrongKind(who: Int) = SayWords(
        Tones.STOP, who, "Not the code expected",
        "This code carries something this step does not take, so nothing was used.",
        "Cancel, then ask for the right code again, or use a file instead.",
        null, null, null, null,
    )

    /** The code was read in full but what it holds was refused. */
    fun contentRefused(who: Int) = SayWords(
        Tones.STOP, who, "Code content not usable",
        "The code was read in full, but what it holds is not in a form this step takes.",
        "Ask for the code to be shown again, or use a file instead.",
        null, null, null, null,
    )

    /** The library gave no usable answer. */
    fun readerFailed(who: Int) = SayWords(
        Tones.STOP, who, "Code reader failed",
        "The part of this app that reads codes gave no usable answer, so nothing was read.",
        "Close the app and open it again. If it happens again, use a file.",
        null, null, null, null,
    )

    /** A code that changes is on the screen. */
    fun showingAnimated(who: Int) = SayWords(
        Tones.INFO, who, "Answer shown as a code",
        "The answer is on the screen as a code that changes, one part after another.",
        "Hold the other phone to the screen until it has read every part.",
        null, null, null, null,
    )

    /** One still code is on the screen. */
    fun showingStill(who: Int) = SayWords(
        Tones.INFO, who, "Answer shown as one code",
        "The answer is on the screen as one code that does not change.",
        "Hold the other phone to the screen until it has read the code.",
        null, null, null, null,
    )

    /** No code could be made of the answer. */
    fun cannotShow(who: Int) = SayWords(
        Tones.STOP, who, "No code could be made",
        "The answer could not be turned into a code, so nothing is shown.",
        "Save the answer as a file instead.",
        null, null, null, null,
    )

    /** The words of reading a code that changes, for the tests and the lists of words. */
    fun forReading(who: Int): List<Pair<String, SayWords>> = listOf(
        "UrWords.reading" to reading(who, 3, 9),
        "UrWords.restarted" to restarted(who),
        "UrWords.otherCode" to otherCode(who),
        "UrWords.wrongKind" to wrongKind(who),
        "UrWords.contentRefused" to contentRefused(who),
        "UrWords.readerFailed" to readerFailed(who),
    )

    /** The words of showing a code that changes. */
    fun forShowing(who: Int): List<Pair<String, SayWords>> = listOf(
        "UrWords.showingAnimated" to showingAnimated(who),
        "UrWords.showingStill" to showingStill(who),
        "UrWords.cannotShow" to cannotShow(who),
    )

    /** Every words of this object. */
    fun all(who: Int): List<Pair<String, SayWords>> = forReading(who) + forShowing(who)
}
