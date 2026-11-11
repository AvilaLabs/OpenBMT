package org.openbmt.app

import android.content.Context
import android.graphics.ImageFormat
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.params.OutputConfiguration
import android.hardware.camera2.params.SessionConfiguration
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.view.Surface
import android.view.SurfaceHolder
import java.util.concurrent.Executor
import java.util.concurrent.atomic.AtomicBoolean
import org.openbmt.common.QrNative

/**
 * Reads one QR code with the back camera, with the platform's Camera2 and nothing else. Frames come
 * from an [ImageReader] in YUV_420_888 near 1280 by 720; only the Y plane is used, with its row stride,
 * and it goes to [QrNative.decode] on a background thread, at most about eight times a second, the
 * latest frame only (the others are dropped). The camera is released on the first code, on [stop]
 * and when something fails. A frame is held only while it is read: it is zeroed afterwards, never
 * written to a file and never sent anywhere.
 *
 * [onCode] and [onError] are called once in all, on the main thread, and not at all after [stop]. In
 * [continuous] mode the camera stays open after a code and [onCode] is called for each code read that
 * differs from the one read just before it, until [stop] or an error; the caller stops it. The parts of
 * an animated code come this way.
 * The camera permission is the caller's to hold before [start].
 */
class QrScanner(
    private val context: Context,
    private val main: Handler,
    private val onCode: (ByteArray) -> Unit,
    private val onError: (String) -> Unit,
    private val continuous: Boolean = false,
) {
    private val lock = Any()
    private var session: Session? = null

    /** What [prepare] found: the camera and the sizes. */
    private class Plan(val cameraId: String, val frame: Pair<Int, Int>, val preview: Pair<Int, Int>?, val autofocus: Boolean)

    private var plan: Plan? = null

    /**
     * Looks at the cameras and returns the size of the preview, or null when the camera has none for a
     * surface view. Throws [IllegalStateException] with a text when there is no usable camera or the
     * reader library does not load.
     */
    fun prepare(): Pair<Int, Int>? {
        QrNative.load()?.let { throw IllegalStateException(it) }
        val manager = context.getSystemService(CameraManager::class.java)
            ?: throw IllegalStateException("no camera service")
        try {
            val ids = manager.cameraIdList
            val id = ids.firstOrNull {
                manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK
            } ?: ids.firstOrNull() ?: throw IllegalStateException("no camera")
            val chars = manager.getCameraCharacteristics(id)
            val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
                ?: throw IllegalStateException("the camera lists no sizes")
            val frame = QrFrames.chooseSize((map.getOutputSizes(ImageFormat.YUV_420_888) ?: emptyArray()).map { it.width to it.height })
                ?: throw IllegalStateException("the camera has no size for frames")
            val preview = QrFrames.chooseSize((map.getOutputSizes(SurfaceHolder::class.java) ?: emptyArray()).map { it.width to it.height })
            val modes = chars.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES)
            val autofocus = modes != null && modes.contains(CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
            plan = Plan(id, frame, preview, autofocus)
            return preview
        } catch (e: CameraAccessException) {
            throw IllegalStateException(e.message ?: "the camera could not be read")
        }
    }

    /** Opens the camera and starts reading; [preview] is drawn on as well when it is not null. Main thread. */
    fun start(preview: Surface?) {
        val p = plan ?: return fail("the camera was not prepared")
        synchronized(lock) {
            if (session != null) return
            session = Session(p, preview)
        }
        session?.open()
    }

    /** Releases the camera and the thread. Safe to call more than once and when nothing started. */
    fun stop() {
        val s = synchronized(lock) {
            val s = session
            session = null
            s
        }
        s?.release()
    }

    private fun fail(text: String) {
        stop()
        main.post { onError(text) }
    }

    private inner class Session(private val plan: Plan, private val preview: Surface?) {
        private val thread = HandlerThread("qr-scan").also { it.start() }
        private val handler = Handler(thread.looper)
        private val executor = Executor { handler.post(it) }
        private val throttle = DecodeThrottle(125)
        private val closed = AtomicBoolean(false)
        private val finished = AtomicBoolean(false)

        /** The last code given in continuous mode; read and written on the reading thread only. */
        private var lastCode: ByteArray? = null
        private var camera: CameraDevice? = null
        private var capture: CameraCaptureSession? = null
        private var reader: ImageReader? = null

        fun open() {
            try {
                val (w, h) = plan.frame
                val r = ImageReader.newInstance(w, h, ImageFormat.YUV_420_888, 2)
                r.setOnImageAvailableListener({ onImage(it) }, handler)
                synchronized(lock) { reader = r }
                val manager = context.getSystemService(CameraManager::class.java)
                    ?: return problem("no camera service")
                manager.openCamera(plan.cameraId, executor, cameraCallback)
            } catch (e: CameraAccessException) {
                problem(e.message ?: "the camera could not be opened")
            } catch (e: SecurityException) {
                problem(e.message ?: "the camera is not allowed")
            } catch (e: RuntimeException) {
                problem(e.message ?: "the camera could not be opened")
            }
        }

        private val cameraCallback = object : CameraDevice.StateCallback() {
            override fun onOpened(device: CameraDevice) {
                synchronized(lock) {
                    if (closed.get()) {
                        device.close()
                        return
                    }
                    camera = device
                }
                try {
                    val outputs = ArrayList<OutputConfiguration>()
                    reader?.let { outputs.add(OutputConfiguration(it.surface)) }
                    preview?.let { outputs.add(OutputConfiguration(it)) }
                    device.createCaptureSession(
                        SessionConfiguration(SessionConfiguration.SESSION_REGULAR, outputs, executor, sessionCallback),
                    )
                } catch (e: CameraAccessException) {
                    problem(e.message ?: "the camera could not start")
                } catch (e: RuntimeException) {
                    problem(e.message ?: "the camera could not start")
                }
            }

            override fun onDisconnected(device: CameraDevice) = problem("the camera was taken by another app")

            override fun onError(device: CameraDevice, error: Int) = problem("the camera reported error $error")
        }

        private val sessionCallback = object : CameraCaptureSession.StateCallback() {
            override fun onConfigured(s: CameraCaptureSession) {
                synchronized(lock) {
                    if (closed.get()) {
                        s.close()
                        return
                    }
                    capture = s
                }
                try {
                    val device = s.device
                    val request = device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
                    reader?.let { request.addTarget(it.surface) }
                    preview?.let { request.addTarget(it) }
                    if (plan.autofocus) {
                        request.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE)
                    }
                    s.setRepeatingRequest(request.build(), null, handler)
                } catch (e: CameraAccessException) {
                    problem(e.message ?: "the camera could not start")
                } catch (e: RuntimeException) {
                    problem(e.message ?: "the camera could not start")
                }
            }

            override fun onConfigureFailed(s: CameraCaptureSession) = problem("the camera could not be set up")
        }

        /** Runs on the background thread for each frame the reader has. Only the latest frame is taken. */
        private fun onImage(r: ImageReader) {
            val image = try {
                r.acquireLatestImage()
            } catch (_: IllegalStateException) {
                null
            } ?: return
            var luma: Luma? = null
            var width = 0
            var height = 0
            try {
                if (closed.get() || finished.get()) return
                if (!throttle.admit(SystemClock.elapsedRealtime())) return
                val y = image.planes[0]
                width = image.width
                height = image.height
                luma = QrFrames.copyLuma(y.buffer, width, height, y.rowStride, y.pixelStride, false)
            } finally {
                image.close()
            }
            val frame = luma ?: return
            val reply = try {
                QrNative.decode(width, height, frame.stride, frame.bytes)
            } finally {
                frame.bytes.fill(0)
            }
            when (val parsed = QrReply.parse(reply)) {
                is QrReply.Decoded -> {
                    if (continuous) {
                        // The same code again and again (the part still in view) is given once.
                        if (closed.get() || finished.get()) return
                        val last = lastCode
                        if (last != null && last.contentEquals(parsed.bytes)) return
                        lastCode = parsed.bytes.copyOf()
                        val bytes = parsed.bytes
                        main.post { if (!closed.get()) onCode(bytes) }
                        return
                    }
                    if (closed.get() || !finished.compareAndSet(false, true)) return
                    release()
                    main.post { onCode(parsed.bytes) }
                }
                is QrReply.Refused -> {}
                is QrReply.Fault -> problem("the reader gave no answer")
            }
        }

        /** Reports a failure once and releases everything. */
        private fun problem(text: String) {
            if (closed.get() || !finished.compareAndSet(false, true)) return
            release()
            main.post { onError(text) }
        }

        /** Closes the camera first, then the reader and the thread after any frame being read. */
        fun release() {
            if (!closed.compareAndSet(false, true)) return
            val (c, d, r) = synchronized(lock) { Triple(capture, camera, reader) }
            try {
                c?.stopRepeating()
            } catch (_: CameraAccessException) {
            } catch (_: IllegalStateException) {
            }
            c?.close()
            d?.close()
            handler.post {
                lastCode?.fill(0)
                lastCode = null
                r?.close()
                thread.quitSafely()
            }
        }
    }
}
