package com.github.catvod.spider;

import android.app.Activity;
import android.app.Application;
import android.content.Context;
import android.os.Handler;
import android.os.Looper;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import org.json.JSONObject;

public class Init {
    private static final String LEGACY_CONFIG =
            "{\"version\":\"29.0\",\"update\":\"关闭\",\"danmuColor\":\"默认\",\"aliHD\":\"阿里原画\",\"quarkHD\":\"夸克原画\",\"ucHD\":\"UC原画\",\"baiduHD\":\"百度原画\",\"123HD\":\"123无限\",\"panBlock\":\"\",\"proxyMode\":\"Java多线程\",\"pansouUrl\":\"https://so.252035.xyz\",\"panOrder\":\"百度,夸克,UC,迅雷,光鸭,天翼,123,阿里,移动\",\"homePage\":\"热门电影,热播剧集,热门动漫,热播综艺,电影筛选,电视筛选,电影榜单,电视剧榜单\",\"aliThread\":\"64\",\"quarkThread\":\"16\",\"ucThread\":\"自动\",\"baiduThread\":\"10\",\"xunleiThread\":\"10\"}";
    private static final String MODERN_CONFIG =
            "{\"版本\":\"32.0\",\"update\":\"关闭\",\"danmuColor\":\"默认\",\"aliQuality\":\"阿里原画\",\"quarkQuality\":\"夸克原画\",\"ucQuality\":\"UC无限\",\"baiduQuality\":\"百度无限\",\"123Quality\":\"123无限\",\"panBlock\":\"\",\"proxyMode\":\"Java多线程\",\"pansouUrl\":\"https://so.252035.xyz\",\"panOrder\":\"夸克,UC,百度,迅雷,光鸭,天翼,123,阿里\",\"homePage\":\"猜你喜欢,热门电影,热播剧集,热门动漫,热播综艺,电影筛选,电视筛选,电影榜单,电视剧榜单\",\"aliThread\":\"64\",\"quarkThread\":\"16\",\"ucThread\":\"自动\",\"baiduThread\":\"10\",\"xunleiThread\":\"10\"}";
    public static String v = "desktop";
    private static final HashMap<String, Boolean> keywords = new HashMap<>();
    private Application c;
    private final Handler b = new Handler(Looper.getMainLooper());
    private final ExecutorService a = Executors.newFixedThreadPool(5, runnable -> {
        Thread thread = new Thread(runnable, "webhtv-init-worker");
        thread.setDaemon(true);
        return thread;
    });

    private static class Loader {
        static final Init a = new Init();
    }

    public static Init get() {
        return Loader.a;
    }

    public static void init(Context context) {
        if (context instanceof Application application) {
            get().c = application;
        } else if (context != null) {
            get().c = new Application(context.getFilesDir().getParentFile());
        } else {
            get().c = new Application(new File(System.getProperty("java.io.tmpdir"), "webhtv-android-root"));
        }
        seedDefaultConfig(get().c);
    }

    private static void seedDefaultConfig(Application application) {
        for (Map.Entry<String, String> entry : Map.of(
                        "config.json", LEGACY_CONFIG,
                        ".config.json", LEGACY_CONFIG,
                        "配置.json", MODERN_CONFIG)
                .entrySet()) {
            String name = entry.getKey();
            try {
                seedConfigFile(new File(application.getFilesDir(), name), new JSONObject(entry.getValue()));
            } catch (Exception error) {
                System.err.println("Unable to seed desktop Spider config " + name + ": " + error.getMessage());
            }
        }
    }

    private static void seedConfigFile(File config, JSONObject defaults) throws Exception {
        File parent = config.getParentFile();
        if (parent != null) parent.mkdirs();
        JSONObject current = config.exists() && config.length() > 0
                ? new JSONObject(Files.readString(config.toPath(), StandardCharsets.UTF_8))
                : new JSONObject();
        boolean changed = !config.exists() || config.length() == 0;
        for (String key : defaults.keySet()) {
            if (!current.has(key)) {
                current.put(key, defaults.get(key));
                changed = true;
            }
        }
        if ("Go多线程".equals(current.optString("proxyMode"))) {
            current.put("proxyMode", "Java多线程");
            changed = true;
        }
        if (changed) Files.writeString(config.toPath(), current.toString(), StandardCharsets.UTF_8);
    }

    public static Application context() {
        if (get().c == null) {
            init(null);
        }
        return get().c;
    }

    public static void execute(Runnable runnable) {
        if (runnable != null) {
            get().a.execute(runnable);
        }
    }

    public static void run(Runnable runnable) {
        get().b.post(runnable);
    }

    public static void run(Runnable runnable, int delay) {
        get().b.postDelayed(runnable, delay);
    }

    public static Activity getActivity() {
        return null;
    }

    public static Activity getConfigActivity() {
        return null;
    }

    public static Map<String, Boolean> getKeywordsMap() {
        return keywords;
    }

    public static void checkPermission() {
    }

    public static void startFloatBall() {
    }

    public static void startGoProxy(Context context) {
    }

    public static void interceptActivityStart() {
    }
}
