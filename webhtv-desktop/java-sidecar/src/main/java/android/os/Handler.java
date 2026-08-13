package android.os;

public class Handler {
    private final Looper looper;

    public Handler() {
        this(Looper.getMainLooper());
    }

    public Handler(Looper looper) {
        this.looper = looper == null ? Looper.getMainLooper() : looper;
    }

    public final boolean post(Runnable runnable) {
        if (runnable != null) {
            runnable.run();
        }
        return true;
    }

    public final boolean postDelayed(Runnable runnable, long delayMillis) {
        if (runnable != null) {
            runnable.run();
        }
        return true;
    }

    public Looper getLooper() {
        return looper;
    }
}
