package android.os;

public final class Process {
    private Process() {
    }

    public static int myPid() {
        return (int) ProcessHandle.current().pid();
    }

    public static void killProcess(int pid) {
    }
}
