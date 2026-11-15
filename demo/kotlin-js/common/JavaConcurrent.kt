// java.util.concurrent for the browser build (see JavaIo.kt). The page has one
// thread, so an executor runs what it is given at once, in the order it was
// given, and a future is already done when it is returned.
@file:Suppress("PackageDirectoryMismatch", "unused")

package java.util.concurrent

fun interface Callable<T> {
    fun call(): T
}

fun interface ThreadFactory {
    fun newThread(r: Runnable): Thread
}

class ExecutionException(cause: Throwable?) : Exception(cause?.message, cause)

class Future<T> internal constructor(private val value: T?, private val failure: Throwable?) {
    fun get(): T {
        if (failure != null) throw ExecutionException(failure)
        @Suppress("UNCHECKED_CAST")
        return value as T
    }
}

class ExecutorService internal constructor(private val factory: ThreadFactory) {
    fun execute(command: Runnable) {
        factory.newThread(command).start()
    }

    fun <T> submit(task: Callable<T>): Future<T> = try {
        Future(task.call(), null)
    } catch (e: Throwable) {
        Future(null, e)
    }
}

object Executors {
    fun newSingleThreadExecutor(factory: ThreadFactory): ExecutorService = ExecutorService(factory)
}
