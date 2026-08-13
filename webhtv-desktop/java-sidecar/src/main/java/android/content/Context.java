package android.content;

import android.content.pm.ApplicationInfo;
import java.io.File;
import java.util.HashMap;
import java.util.Map;

public class Context {
    private final File filesDir;
    private final File cacheDir;
    private final File externalFilesDir;
    private final ApplicationInfo applicationInfo = new ApplicationInfo();
    private final Map<String, SharedPreferences> preferences = new HashMap<>();

    public Context(File root) {
        this.filesDir = new File(root, "files");
        this.cacheDir = new File(root, "cache");
        this.externalFilesDir = new File(root, "external");
        this.filesDir.mkdirs();
        this.cacheDir.mkdirs();
        this.externalFilesDir.mkdirs();
    }

    public File getFilesDir() {
        return filesDir;
    }

    public File getCacheDir() {
        return cacheDir;
    }

    public File getExternalFilesDir(String type) {
        File dir = type == null || type.isEmpty() ? externalFilesDir : new File(externalFilesDir, type);
        dir.mkdirs();
        return dir;
    }

    public Context getApplicationContext() {
        return this;
    }

    public String getPackageName() {
        return "com.fongmi.android.tv";
    }

    public synchronized SharedPreferences getSharedPreferences(String name, int mode) {
        String safeName = name == null || name.isEmpty() ? "default" : name.replaceAll("[^a-zA-Z0-9._-]", "_");
        return preferences.computeIfAbsent(
                safeName,
                key -> new SimpleSharedPreferences(new File(filesDir, "shared-prefs-" + key + ".json")));
    }

    public Object getSystemService(String name) {
        if ("connectivity".equals(name)) {
            return new android.net.ConnectivityManager();
        }
        return null;
    }

    public int checkCallingOrSelfPermission(String permission) {
        return 0;
    }

    public ApplicationInfo getApplicationInfo() {
        return applicationInfo;
    }
}
