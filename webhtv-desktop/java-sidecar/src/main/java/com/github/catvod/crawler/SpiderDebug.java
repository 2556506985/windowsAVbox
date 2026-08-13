package com.github.catvod.crawler;

public final class SpiderDebug {
    private SpiderDebug() {
    }

    public static boolean isEnabled() {
        return true;
    }

    public static void log(Throwable th) {
        if (th != null) {
            th.printStackTrace(System.err);
        }
    }

    public static void log(String tag, Throwable th) {
        if (tag != null) {
            System.err.println(tag);
        }
        log(th);
    }

    public static void log(String msg) {
        if (msg != null && !msg.isEmpty()) {
            System.err.println(msg);
        }
    }

    public static void log(String tag, String msg, Object... args) {
        if (msg == null) {
            return;
        }
        try {
            System.err.println(tag + ": " + (args == null || args.length == 0 ? msg : String.format(msg, args)));
        } catch (Throwable ignored) {
            System.err.println(tag + ": " + msg);
        }
    }
}
