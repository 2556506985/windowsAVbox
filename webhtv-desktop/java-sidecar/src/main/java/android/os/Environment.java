package android.os;

import java.io.File;

public final class Environment {
    private static File externalStorageDirectory = new File(System.getProperty("java.io.tmpdir"), "webhtv-external");

    private Environment() {
    }

    public static void setExternalStorageDirectory(File directory) {
        externalStorageDirectory = directory;
        externalStorageDirectory.mkdirs();
    }

    public static File getExternalStorageDirectory() {
        externalStorageDirectory.mkdirs();
        return externalStorageDirectory;
    }
}
