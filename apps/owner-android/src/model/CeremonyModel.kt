package org.openbmt.owner.app

import org.openbmt.app.KeyTextOutcome
import org.openbmt.app.UrWords
import org.openbmt.owner.PairView

/** What an unfinished setup offers next, from the stage the library reports for its sealed record. */
enum class ResumeAction(val label: String) {
    /** Stage 1: key 1 is made; the Key app's answer is next. */
    ScanAnswer("Scan the Key app's code"),

    /** Stage 2: the wallet was built; the setup for the Key app is not kept, so the answer is read again. */
    ScanAnswerAgain("Scan the Key app's code again"),

    /** Stage 3: the owner confirmed; saving the setup is what is left. */
    SaveSetup("Save the setup"),
}

/**
 * What the screens that make a new shop wallet write and decide, as plain text and plain values (OA-26 to
 * OA-31). Everything that has a meaning is the library's: the keys, the answer, the wallet, the words and the
 * stages (OC-40 to OC-46, SR-64 to SR-69). The numbers the last two steps are given (the batch minimum, the
 * refund depth and the first block) are the library's too, asked from it and copied nowhere. No Android class
 * is used.
 */
object CeremonyModel {
    /** The Key app's answer is about 200 bytes; a code that holds more than this is not one. */
    const val MAX_ANSWER_BYTES = 1_024

    /** A key 2 text, `[fingerprint/path]key`, is about 160 characters. */
    const val MAX_KEY2_TEXT = 400

    /** The types of code the scan of the Key app's answer takes: its `ur:bytes` and nothing else. */
    val ANSWER_TYPES: Set<String> = ExchangeModel.RESPONSE_TYPES

    /** The types of UR the scan of key 2 takes besides a plain text code (OA-28). */
    val KEY2_TYPES: Set<String> = setOf("crypto-account", "crypto-hdkey", "bytes")

    /** The four words, written large to compare. */
    fun wordsLine(words: List<String>): String = CounterModel.wordsLine(words)

    /** One line for each key's fingerprint, in the library's order: keys 1, 2 and 3. */
    fun fingerprintLines(fingerprints: List<String>): List<String> =
        fingerprints.mapIndexed { i, f -> "Key ${i + 1}: $f" }

    /** Who holds a key, as the library numbers the holders. */
    fun holder(code: Int): String = when (code) {
        1 -> "this phone"
        2 -> "the Key app"
        3 -> "another signer"
        4 -> "another wallet app"
        else -> "an unknown holder"
    }

    /** One line for each pair of keys and who holds each, in the library's order. */
    fun pairLines(pairs: List<PairView>): List<String> =
        pairs.map { "Keys ${it.keyA} and ${it.keyB}: ${holder(it.holderA)} and ${holder(it.holderB)}" }

    /** What the scan of the Key app's answer gave, made into the same two results the other sources give. */
    fun answerResult(payload: ByteArray): SourceResult =
        if (payload.size > MAX_ANSWER_BYTES) SourceResult.Failed(AppWords.ceremonyCodeTooLong) else SourceResult.Bytes(payload)

    /** Key 2 as typed or pasted: only the length is checked here, and the library reads the key (SR-64). */
    fun key2Text(text: String): Form<String> {
        val t = text.trim()
        return if (t.isEmpty() || t.length > MAX_KEY2_TEXT) Form.Refused(AppWords.ceremonyKey2Text) else Form.Ok(t)
    }

    /** Key 2 as a scanned plain code: its bytes are the text, and a code that is not text is refused. */
    fun key2FromScan(bytes: ByteArray): Form<String> {
        val text = try {
            Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes)).toString()
        } catch (_: java.nio.charset.CharacterCodingException) {
            return Form.Refused(AppWords.ceremonyKey2Text)
        }
        return key2Text(text)
    }

    /**
     * Key 2 from what the library made of a finished UR (UR-31): the text goes on as scanned text does, a code that
     * is not taken as a key shows the library's words, and a request or answer outside the framing shows the words
     * of a reader that failed. Either way the key 2 screen is where the owner is.
     */
    fun key2FromMessage(outcome: KeyTextOutcome): Form<String> = when (outcome) {
        is KeyTextOutcome.Key -> key2Text(outcome.text)
        is KeyTextOutcome.Declined -> Form.Refused(UrWords.keyDeclined(ExchangeModel.WHO, outcome))
        is KeyTextOutcome.Refused, KeyTextOutcome.Fault -> Form.Refused(UrWords.readerFailed(ExchangeModel.WHO))
    }

    /** The code for the Key app: with a code made ([made]), of several parts or one, or else the words of why not. */
    fun setupCodeStep(made: Boolean, animated: Boolean): ExchangeStep.Code = when {
        !made -> ExchangeStep.Code(AppWords.ceremonyCodeNotMade, false)
        animated -> ExchangeStep.Code(AppWords.ceremonyCodeAnimated, true)
        else -> ExchangeStep.Code(AppWords.ceremonyCodeStill, true)
    }

    /** What an unfinished setup offers at the library's [stage], or null for a stage this app does not know. */
    fun resumeAction(stage: Int): ResumeAction? = when (stage) {
        1 -> ResumeAction.ScanAnswer
        2 -> ResumeAction.ScanAnswerAgain
        3 -> ResumeAction.SaveSetup
        else -> null
    }

    /** The words that go with a stage that is resumed, besides the library's words of the stage. */
    fun resumeWords(action: ResumeAction) = when (action) {
        ResumeAction.ScanAnswer -> AppWords.ceremonyScanNext
        ResumeAction.ScanAnswerAgain -> AppWords.ceremonyScanAgain
        ResumeAction.SaveSetup -> AppWords.ceremonySaveNext
    }
}
