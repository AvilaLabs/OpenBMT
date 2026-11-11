package org.openbmt.owner.app

import org.openbmt.app.CameraWords
import org.openbmt.app.SayWords
import org.openbmt.app.Who

/** The three screens of the approval exchange with the Key app, in the order the owner goes through them. */
sealed class ExchangeStep {
    /** The request is ready: the ways to give it to the Key app. */
    object Request : ExchangeStep()

    /**
     * The request as a code on the screen. [words] say what to do next, or why there is no code; [shown] is
     * whether a code is on the screen. Done is offered only with a code on the screen.
     */
    class Code(val words: SayWords, val shown: Boolean) : ExchangeStep() {
        val done: Boolean get() = shown
    }

    /** Bringing back the Key app's response; [message] is what the last try said, if it said anything. */
    class Response(val message: SayWords?) : ExchangeStep()
}

/** What the request screen offers, in the order it offers it. */
enum class RequestAction(val label: String) {
    ShowCode("Show as code"),
    SaveFile("Save the request as a file"),
    CopyText("Copy the request as text"),
    Response("Bring back the response"),
}

/** The ways the response comes back, in the order the screen offers them. */
enum class ResponseKind { Scan, File, Pasted }

/**
 * The exchange with the Key app by camera and screen. The request is shown as `ur:bytes` parts, as the
 * Key app shows its own answer, and the response is scanned from the Key app's code. The type and the
 * share of the screen are the Key app's (`TYPE_BYTES`, 60% of the screen's height in its Controller);
 * [WHO] is the person who does the next step in this app.
 */
object ExchangeModel {
    /** OpenBMT's own framed requests and responses travel as `ur:bytes`. */
    const val TYPE_BYTES = "bytes"

    /** The code is drawn at most this share of the screen's height, as the Key app draws its answer. */
    const val CODE_HEIGHT_SHARE = 0.6

    const val WHO = Who.OWNER

    /** The types of code the response scan takes: the Key app's `ur:bytes` and nothing else. */
    val RESPONSE_TYPES: Set<String> = setOf(TYPE_BYTES)

    val requestActions: List<RequestAction> = listOf(
        RequestAction.ShowCode,
        RequestAction.SaveFile,
        RequestAction.CopyText,
        RequestAction.Response,
    )

    val responseKinds: List<ResponseKind> = listOf(ResponseKind.Scan, ResponseKind.File, ResponseKind.Pasted)

    /** The height in pixels of the code on a screen of [screenHeightPx]. */
    fun codeHeight(screenHeightPx: Int): Int = (screenHeightPx * CODE_HEIGHT_SHARE).toInt()

    /** The code screen: with a code made ([made]), of several parts or one, or else the words of why not. */
    fun codeStep(made: Boolean, animated: Boolean): ExchangeStep.Code = when {
        !made -> ExchangeStep.Code(AppWords.requestCodeNotMade, false)
        animated -> ExchangeStep.Code(AppWords.requestCodeAnimated, true)
        else -> ExchangeStep.Code(AppWords.requestCodeStill, true)
    }
}

/** What a source gave: bytes for the library to judge, or the words of why there are none. */
sealed class SourceResult {
    class Bytes(val bytes: ByteArray) : SourceResult()

    class Failed(val words: SayWords) : SourceResult()
}

/**
 * What the scan of the Key app's code gives back, made into the same two results the other sources give.
 * Nothing is judged here but the length and the kind of code: what the bytes mean is the library's (OA-07).
 */
object ScanResult {
    /** A complete `ur:bytes` message: its payload goes on as bytes, unless it is longer than a response can be. */
    fun message(payload: ByteArray): SourceResult =
        if (payload.size > ApprovalText.MAX_BYTES) {
            SourceResult.Failed(AppWords.responseNotReadable("the code holds more than a response can be"))
        } else {
            SourceResult.Bytes(payload)
        }

    /** A code that is not a UR: the Key app does not show its response so. */
    fun plain(): SourceResult = SourceResult.Failed(AppWords.scanNotAResponse)

    /** The scan was cancelled, or its permission dialog was dismissed. */
    fun cancelled(): SourceResult = SourceResult.Failed(AppWords.scanCancelled)

    /**
     * A scan that did not happen or stopped: the words the reading gave ([say]: a wrong type of code, a code
     * of something this step does not take, a reader that failed), else the camera is off for the app
     * ([off]), else the camera did not start ([problem] is the system's text).
     */
    fun failed(off: Boolean, problem: String?, say: SayWords?): SourceResult = SourceResult.Failed(
        say ?: if (off) CameraWords.off(ExchangeModel.WHO) else CameraWords.notStarted(ExchangeModel.WHO, problem),
    )
}
