package com.fongmi.webhtv.java;

import android.app.Application;
import android.content.Context;
import android.os.Environment;
import com.github.catvod.crawler.Spider;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.net.URI;
import java.net.URL;
import java.net.URLClassLoader;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;
import java.util.zip.ZipFile;
import org.json.JSONArray;
import org.json.JSONObject;

final class JarRuntime implements AutoCloseable {
    private static final long MAX_REMOTE_JAR_BYTES = 100L * 1024L * 1024L;
    private static final Duration CONNECT_TIMEOUT = Duration.ofSeconds(10);
    private static final Duration REQUEST_TIMEOUT = Duration.ofSeconds(60);
    private static final Duration READ_TIMEOUT = Duration.ofSeconds(30);
    private static final HttpClient HTTP_CLIENT = HttpClient.newBuilder()
            .connectTimeout(CONNECT_TIMEOUT)
            .followRedirects(HttpClient.Redirect.NORMAL)
            .build();
    private static final Map<String, String> MODERN_CONFIG_KEYS = Map.of(
            "aliHD", "aliQuality",
            "quarkHD", "quarkQuality",
            "ucHD", "ucQuality",
            "baiduHD", "baiduQuality",
            "123HD", "123Quality");

    private final Path workDir;
    private final DexConverter converter;
    private final Application application;
    private final ConcurrentHashMap<String, URLClassLoader> loaders = new ConcurrentHashMap<>();
    private final ConcurrentHashMap<String, Spider> spiders = new ConcurrentHashMap<>();
    private final ConcurrentHashMap<String, Method> proxyMethods = new ConcurrentHashMap<>();
    private final ProxyServer proxyServer;
    private final CloudAuthManager cloudAuth;
    private volatile String recentJarKey;

    JarRuntime(Path workDir) throws Exception {
        this.workDir = workDir;
        FilesMkdirs(workDir);
        this.converter = new DexConverter(workDir.resolve("dex-cache"));
        File appRoot = workDir.resolve("android-root").toFile();
        this.application = new Application(appRoot);
        Environment.setExternalStorageDirectory(workDir.resolve("external-storage").toFile());
        this.proxyServer = new ProxyServer(this::proxy);
        this.cloudAuth = new CloudAuthManager(workDir);
    }

    synchronized Spider ensureSpider(String siteKey, String api, String ext, String jar) throws Exception {
        if (jar == null || jar.isBlank()) {
            throw new IllegalArgumentException("Java Spider requires a jar path");
        }
        if (api == null || !api.startsWith("csp_")) {
            throw new IllegalArgumentException("Java Spider api must start with csp_");
        }
        Path source = resolveJar(jar);
        String jarKey = source.toAbsolutePath().normalize().toString();
        String spiderKey = jarKey + "\0" + siteKey + "\0" + api + "\0" + (ext == null ? "" : ext);
        Spider cached = spiders.get(spiderKey);
        if (cached != null) {
            recentJarKey = jarKey;
            return cached;
        }
        URLClassLoader loader = loaders.computeIfAbsent(jarKey, key -> {
            try {
                Path converted = converter.convert(source);
                return new URLClassLoader(new URL[] {converted.toUri().toURL()}, JarRuntime.class.getClassLoader());
            } catch (Exception error) {
                throw new RuntimeException(error);
            }
        });
        invokeJarInit(loader);
        registerProxy(jarKey, loader);
        recentJarKey = jarKey;
        String className = "com.github.catvod.spider." + api.substring("csp_".length());
        Class<?> type = Class.forName(className, true, loader);
        Constructor<?> constructor = type.getDeclaredConstructor();
        constructor.setAccessible(true);
        Object instance = constructor.newInstance();
        if (!(instance instanceof Spider spider)) {
            throw new IllegalStateException(className + " is not a Spider");
        }
        spider.siteKey = siteKey;
        invokeInit(spider, ext == null ? "" : ext);
        spiders.put(spiderKey, spider);
        return spider;
    }

    synchronized void setConfig(String key, String value) throws Exception {
        if (key == null || key.isBlank()) throw new IllegalArgumentException("config key is empty");
        String normalizedValue = value == null ? "" : value;
        String modernKey = MODERN_CONFIG_KEYS.getOrDefault(key, key);
        String legacyKey = legacyConfigKey(key);
        writeConfig(application.getFilesDir().toPath().resolve("配置.json"), modernKey, normalizedValue);
        writeConfig(application.getFilesDir().toPath().resolve("config.json"), legacyKey, normalizedValue);
    }

    synchronized String getConfig(String key) throws Exception {
        if (key == null || key.isBlank()) throw new IllegalArgumentException("config key is empty");
        String modernKey = MODERN_CONFIG_KEYS.getOrDefault(key, key);
        String value = readConfig(application.getFilesDir().toPath().resolve("配置.json"), modernKey);
        if (!value.isEmpty()) return value;
        return readConfig(
                application.getFilesDir().toPath().resolve("config.json"), legacyConfigKey(key));
    }

    synchronized String cloudAuth(String operation, String provider, String sessionId) throws Exception {
        URLClassLoader loader = recentJarKey == null ? null : loaders.get(recentJarKey);
        return cloudAuth.handle(operation, provider, sessionId, loader).toString();
    }

    private static void writeConfig(Path file, String key, String value) throws Exception {
        JSONObject config = Files.exists(file)
                ? new JSONObject(Files.readString(file, StandardCharsets.UTF_8))
                : new JSONObject();
        config.put(key, value);
        Files.createDirectories(file.getParent());
        Files.writeString(file, config.toString(), StandardCharsets.UTF_8);
    }

    private static String readConfig(Path file, String key) throws Exception {
        if (!Files.exists(file)) return "";
        return new JSONObject(Files.readString(file, StandardCharsets.UTF_8)).optString(key, "");
    }

    private static String legacyConfigKey(String key) {
        for (Map.Entry<String, String> entry : MODERN_CONFIG_KEYS.entrySet()) {
            if (entry.getValue().equals(key)) return entry.getKey();
        }
        return key;
    }

    synchronized Object parse(
            int parserType,
            String parserKey,
            String parserName,
            String flag,
            String url,
            JSONObject parserMap)
            throws Exception {
        URLClassLoader loader = loaders.get(recentJarKey);
        if (loader == null) throw new IllegalStateException("No jar loaded for parser key: " + parserKey);
        LinkedHashMap<String, String> jsonParsers = stringMap(parserMap);
        if (parserType == 2) {
            Class<?> type = Class.forName("com.github.catvod.parser.Json" + parserKey, true, loader);
            Method method = type.getMethod("parse", LinkedHashMap.class, String.class);
            return method.invoke(null, jsonParsers, url);
        }
        if (parserType == 3) {
            Class<?> type = Class.forName("com.github.catvod.parser.Mix" + parserKey, true, loader);
            Method method = type.getMethod(
                    "parse", LinkedHashMap.class, String.class, String.class, String.class);
            return method.invoke(null, mixMap(parserMap), parserName, flag, url);
        }
        throw new IllegalArgumentException("unsupported JAR parser type " + parserType);
    }

    synchronized Object invoke(Spider spider, String method, JSONArray args) throws Exception {
        return switch (method) {
            case "homeContent" -> spider.homeContent(boolArg(args, 0, false));
            case "homeVideoContent" -> spider.homeVideoContent();
            case "categoryContent" -> spider.categoryContent(
                    stringArg(args, 0),
                    stringArg(args, 1, "1"),
                    boolArg(args, 2, false),
                    mapArg(args, 3));
            case "detailContent" -> spider.detailContent(stringListArg(args, 0));
            case "searchContent" -> {
                if (args != null && args.length() >= 3) {
                    yield spider.searchContent(stringArg(args, 0), boolArg(args, 1, false), stringArg(args, 2));
                }
                yield spider.searchContent(stringArg(args, 0), boolArg(args, 1, false));
            }
            case "playerContent" -> spider.playerContent(
                    stringArg(args, 0), stringArg(args, 1), stringListArg(args, 2));
            case "liveContent" -> spider.liveContent(stringArg(args, 0));
            case "manualVideoCheck" -> spider.manualVideoCheck();
            case "isVideoFormat" -> spider.isVideoFormat(stringArg(args, 0));
            case "action" -> spider.action(stringArg(args, 0));
            case "destroy" -> {
                spider.destroy();
                yield null;
            }
            default -> throw new IllegalArgumentException("unsupported Spider method `" + method + "`");
        };
    }

    @Override
    public synchronized void close() {
        for (Spider spider : spiders.values()) {
            try {
                spider.destroy();
            } catch (Throwable ignored) {
            }
        }
        spiders.clear();
        for (URLClassLoader loader : loaders.values()) {
            try {
                loader.close();
            } catch (Throwable ignored) {
            }
        }
        loaders.clear();
        proxyMethods.clear();
        proxyServer.close();
    }

    private Object[] proxy(Map<String, String> params) throws Exception {
        String siteKey = params.get("siteKey");
        if (siteKey != null) {
            for (Spider spider : spiders.values()) {
                if (!siteKey.equals(spider.siteKey)) continue;
                Object[] result = invokeProxy(spider, params);
                if (result != null) return result;
            }
        }
        String recent = recentJarKey;
        if (recent != null) {
            Object[] result = invokeProxy(proxyMethods.get(recent), params);
            if (result != null) return result;
        }
        for (Method method : proxyMethods.values()) {
            Object[] result = invokeProxy(method, params);
            if (result != null) return result;
        }
        return null;
    }

    private void registerProxy(String jarKey, URLClassLoader loader) {
        if (proxyMethods.containsKey(jarKey)) return;
        try {
            Class<?> type = Class.forName("com.github.catvod.spider.Proxy", true, loader);
            proxyMethods.put(jarKey, type.getMethod("proxy", Map.class));
        } catch (Throwable error) {
            System.err.println("jar proxy method unavailable for " + jarKey + ": " + rootMessage(error));
        }
    }

    private static Object[] invokeProxy(Spider spider, Map<String, String> params) {
        try {
            return spider.proxy(params);
        } catch (Throwable error) {
            System.err.println("spider proxy failed: " + rootMessage(error));
            return null;
        }
    }

    private static Object[] invokeProxy(Method method, Map<String, String> params) {
        if (method == null) return null;
        try {
            return (Object[]) method.invoke(null, params);
        } catch (Throwable error) {
            System.err.println("jar proxy failed: " + rootMessage(error));
            error.printStackTrace();
            return null;
        }
    }

    private void invokeJarInit(URLClassLoader loader) {
        try {
            Class<?> initType = Class.forName("com.github.catvod.spider.Init", true, JarRuntime.class.getClassLoader());
            Method method = initType.getMethod("init", Context.class);
            method.invoke(null, application);
            System.err.println("desktop Init bound for " + loader);
        } catch (Throwable error) {
            System.err.println("jar Init skipped: " + rootMessage(error));
        }
    }

    private void invokeInit(Spider spider, String ext) throws Exception {
        if (initializeWogg(spider, ext)) return;
        try {
            Method init = spider.getClass().getMethod("init", Context.class, String.class);
            invokeChecked(init, spider, application, ext);
            return;
        } catch (NoSuchMethodException ignored) {
        }
        try {
            Method init = spider.getClass().getMethod("init", Context.class);
            invokeChecked(init, spider, application);
        } catch (NoSuchMethodException ignored) {
            spider.init(application, ext);
        }
    }

    private boolean initializeWogg(Spider spider, String ext) throws Exception {
        if (!"com.github.catvod.spider.Wogg".equals(spider.getClass().getName())) return false;
        JSONArray configured = new JSONObject(ext).optJSONArray("site");
        if (configured == null || configured.isEmpty()) {
            throw new IllegalArgumentException("Wogg extension does not contain a site URL");
        }

        List<String> sites = new ArrayList<>();
        List<CompletableFuture<String>> probes = new ArrayList<>();
        for (int index = 0; index < configured.length(); index++) {
            String site = configured.optString(index, "").trim();
            if (site.isEmpty()) continue;
            sites.add(site);
            try {
                HttpRequest request = HttpRequest.newBuilder(URI.create(site))
                        .timeout(Duration.ofSeconds(5))
                        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
                        .method("HEAD", HttpRequest.BodyPublishers.noBody())
                        .build();
                probes.add(HTTP_CLIENT.sendAsync(request, HttpResponse.BodyHandlers.discarding())
                        .thenApply(response -> response.statusCode() >= 200 && response.statusCode() < 400
                                ? site
                                : "")
                        .completeOnTimeout("", 5, TimeUnit.SECONDS)
                        .exceptionally(error -> ""));
            } catch (IllegalArgumentException ignored) {
                probes.add(CompletableFuture.completedFuture(""));
            }
        }
        if (sites.isEmpty()) throw new IllegalArgumentException("Wogg extension contains no valid site URL");

        CompletableFuture.allOf(probes.toArray(CompletableFuture[]::new)).join();
        String selected = probes.stream()
                .map(CompletableFuture::join)
                .filter(value -> !value.isEmpty())
                .findFirst()
                .orElse(sites.get(0));
        Field field = spider.getClass().getDeclaredField("f");
        field.setAccessible(true);
        field.set(spider, selected);
        System.err.println("desktop Wogg endpoint selected: " + selected);
        return true;
    }

    private static LinkedHashMap<String, String> stringMap(JSONObject object) {
        LinkedHashMap<String, String> result = new LinkedHashMap<>();
        if (object == null) return result;
        for (String key : object.keySet()) {
            Object value = object.opt(key);
            if (value != null && value != JSONObject.NULL) result.put(key, String.valueOf(value));
        }
        return result;
    }

    private static LinkedHashMap<String, HashMap<String, String>> mixMap(JSONObject object) {
        LinkedHashMap<String, HashMap<String, String>> result = new LinkedHashMap<>();
        if (object == null) return result;
        for (String key : object.keySet()) {
            Object value = object.opt(key);
            if (value instanceof JSONObject parser) {
                result.put(key, new HashMap<>(stringMap(parser)));
            }
        }
        return result;
    }

    private static void invokeChecked(Method method, Object target, Object... args) throws Exception {
        try {
            method.invoke(target, args);
        } catch (java.lang.reflect.InvocationTargetException error) {
            Throwable cause = error.getCause() == null ? error : error.getCause();
            if (cause instanceof Exception exception) {
                throw exception;
            }
            if (cause instanceof Error fatal) {
                throw fatal;
            }
            throw new Exception(cause);
        }
    }

    private Path resolveJar(String jar) throws IOException {
        JarReference reference = parseJarReference(jar);
        if (isHttpUrl(reference.source())) {
            return resolveRemoteJar(reference);
        }

        Path local = resolveLocalJar(reference.source());
        if (!Files.exists(local)) {
            throw new IOException("Spider JAR does not exist: " + local);
        }
        if (!Files.isRegularFile(local)) {
            throw new IOException("Spider JAR is not a regular file: " + local);
        }
        if (reference.md5() != null) {
            verifyMd5(local, reference.md5(), "Local Spider JAR");
        }
        return local;
    }

    private Path resolveRemoteJar(JarReference reference) throws IOException {
        URI uri;
        try {
            uri = URI.create(reference.source());
            if (uri.getHost() == null) {
                throw new IllegalArgumentException("host is missing");
            }
        } catch (IllegalArgumentException error) {
            throw new IOException("Invalid remote Spider JAR URL: " + reference.source(), error);
        }

        Path cacheDir = workDir.resolve("jar-cache");
        Files.createDirectories(cacheDir);
        String cacheKey = sha256(reference.source());
        Path cached = cacheDir.resolve(cacheKey + ".jar");
        if (Files.isRegularFile(cached)) {
            try {
                validateRemoteJar(cached, reference.md5(), reference.source());
                return cached;
            } catch (IOException ignored) {
                // Keep the old file in place until a replacement has been fully validated.
            }
        }

        Path temp = Files.createTempFile(cacheDir, cacheKey + "-", ".tmp");
        try {
            downloadRemoteJar(uri, temp);
            validateRemoteJar(temp, reference.md5(), reference.source());
            try {
                Files.move(
                        temp,
                        cached,
                        StandardCopyOption.ATOMIC_MOVE,
                        StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException error) {
                throw new IOException("Spider JAR cache does not support atomic replacement: " + cacheDir, error);
            }
            return cached;
        } finally {
            Files.deleteIfExists(temp);
        }
    }

    private static void downloadRemoteJar(URI uri, Path destination) throws IOException {
        HttpRequest request;
        try {
            request = HttpRequest.newBuilder(uri)
                    .timeout(REQUEST_TIMEOUT)
                    .header("User-Agent", "WebHTV-Java-Sidecar/0.1")
                    .GET()
                    .build();
        } catch (IllegalArgumentException error) {
            throw new IOException("Invalid remote Spider JAR URL: " + uri, error);
        }

        HttpResponse<InputStream> response;
        try {
            response = HTTP_CLIENT.send(request, HttpResponse.BodyHandlers.ofInputStream());
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new IOException("Interrupted while downloading remote Spider JAR: " + uri, error);
        }

        try (InputStream input = response.body()) {
            int status = response.statusCode();
            if (status < 200 || status >= 300) {
                throw new IOException("Remote Spider JAR request returned HTTP " + status + ": " + uri);
            }
            long contentLength = response.headers().firstValueAsLong("Content-Length").orElse(-1L);
            if (contentLength > MAX_REMOTE_JAR_BYTES) {
                throw new IOException("Remote Spider JAR exceeds the 100 MiB limit: " + uri);
            }
            copyRemoteJar(input, destination, contentLength, uri);
        }
    }

    private static void copyRemoteJar(InputStream input, Path destination, long contentLength, URI uri)
            throws IOException {
        AtomicBoolean timedOut = new AtomicBoolean();
        AtomicLong readDeadline = new AtomicLong(System.nanoTime() + READ_TIMEOUT.toNanos());
        ScheduledExecutorService watchdog = Executors.newSingleThreadScheduledExecutor(runnable -> {
            Thread thread = new Thread(runnable, "webhtv-jar-read-timeout");
            thread.setDaemon(true);
            return thread;
        });
        ScheduledFuture<?> timeout = watchdog.scheduleAtFixedRate(
                () -> {
                    if (System.nanoTime() - readDeadline.get() < 0 || !timedOut.compareAndSet(false, true)) {
                        return;
                    }
                    try {
                        input.close();
                    } catch (IOException ignored) {
                    }
                },
                1,
                1,
                TimeUnit.SECONDS);

        long total = 0;
        try (OutputStream output = Files.newOutputStream(
                destination, StandardOpenOption.WRITE, StandardOpenOption.TRUNCATE_EXISTING)) {
            byte[] buffer = new byte[8192];
            int count;
            while ((count = input.read(buffer)) >= 0) {
                if (count == 0) continue;
                readDeadline.set(System.nanoTime() + READ_TIMEOUT.toNanos());
                total += count;
                if (total > MAX_REMOTE_JAR_BYTES) {
                    throw new IOException("Remote Spider JAR exceeds the 100 MiB limit: " + uri);
                }
                output.write(buffer, 0, count);
            }
        } catch (IOException error) {
            if (timedOut.get()) {
                throw new IOException("Timed out while reading remote Spider JAR: " + uri, error);
            }
            throw error;
        } finally {
            timeout.cancel(false);
            watchdog.shutdownNow();
        }

        if (timedOut.get()) {
            throw new IOException("Timed out while reading remote Spider JAR: " + uri);
        }
        if (contentLength >= 0 && total != contentLength) {
            throw new IOException(
                    "Remote Spider JAR ended early: expected " + contentLength + " bytes but received " + total);
        }
    }

    private static void validateRemoteJar(Path jar, String expectedMd5, String source) throws IOException {
        if (!Files.isRegularFile(jar)) {
            throw new IOException("Remote Spider JAR cache is not a regular file: " + jar);
        }
        long size = Files.size(jar);
        if (size == 0) {
            throw new IOException("Remote Spider JAR is empty: " + source);
        }
        if (size > MAX_REMOTE_JAR_BYTES) {
            throw new IOException("Remote Spider JAR exceeds the 100 MiB limit: " + source);
        }
        try (ZipFile archive = new ZipFile(jar.toFile())) {
            if (!archive.entries().hasMoreElements()) {
                throw new IOException("Remote Spider JAR contains no ZIP entries: " + source);
            }
        } catch (IOException error) {
            if (error.getMessage() != null && error.getMessage().startsWith("Remote Spider JAR contains no")) {
                throw error;
            }
            IOException invalid = new IOException("Remote Spider JAR is not a readable ZIP/JAR: " + source);
            invalid.addSuppressed(error);
            throw invalid;
        }
        if (expectedMd5 != null) {
            verifyMd5(jar, expectedMd5, "Remote Spider JAR");
        }
    }

    private static void verifyMd5(Path jar, String expected, String label) throws IOException {
        String actual = digest(jar, "MD5");
        if (!MessageDigest.isEqual(
                HexFormat.of().parseHex(expected), HexFormat.of().parseHex(actual))) {
            throw new IOException(label + " MD5 mismatch: expected " + expected + " but got " + actual);
        }
    }

    private static String digest(Path path, String algorithm) throws IOException {
        try {
            MessageDigest digest = MessageDigest.getInstance(algorithm);
            try (InputStream input = Files.newInputStream(path)) {
                byte[] buffer = new byte[8192];
                int count;
                while ((count = input.read(buffer)) >= 0) {
                    if (count > 0) digest.update(buffer, 0, count);
                }
            }
            return HexFormat.of().formatHex(digest.digest());
        } catch (NoSuchAlgorithmException error) {
            throw new IOException("Unable to hash Spider JAR with " + algorithm, error);
        }
    }

    private static String sha256(String value) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            return HexFormat.of().formatHex(digest.digest(value.getBytes(StandardCharsets.UTF_8)));
        } catch (NoSuchAlgorithmException error) {
            throw new IllegalStateException("SHA-256 is unavailable", error);
        }
    }

    private static JarReference parseJarReference(String jar) throws IOException {
        String cleaned = jar == null ? "" : jar.trim();
        if (cleaned.isEmpty()) {
            throw new IOException("Spider JAR path is empty");
        }
        int marker = cleaned.toLowerCase(Locale.ROOT).indexOf(";md5;");
        if (marker < 0) {
            return new JarReference(cleaned, null);
        }

        String source = cleaned.substring(0, marker).trim();
        String md5 = cleaned.substring(marker + ";md5;".length()).trim();
        if (source.isEmpty()) {
            throw new IOException("Spider JAR path before ;md5; is empty");
        }
        if (isHttpUrl(md5)) {
            throw new IOException("Remote MD5 URLs are not supported; provide a fixed 32-character hex digest");
        }
        if (md5.length() != 32 || !md5.chars().allMatch(JarRuntime::isHexDigit)) {
            throw new IOException("Invalid Spider JAR MD5; expected exactly 32 hexadecimal characters");
        }
        return new JarReference(source, md5);
    }

    private static boolean isHexDigit(int value) {
        return (value >= '0' && value <= '9')
                || (value >= 'a' && value <= 'f')
                || (value >= 'A' && value <= 'F');
    }

    private static boolean isHttpUrl(String value) {
        return value != null
                && (value.regionMatches(true, 0, "http://", 0, "http://".length())
                        || value.regionMatches(true, 0, "https://", 0, "https://".length()));
    }

    private static Path resolveLocalJar(String source) throws IOException {
        String cleaned = source;
        if (cleaned.startsWith("file:")) {
            try {
                return Path.of(URI.create(cleaned)).toAbsolutePath().normalize();
            } catch (IllegalArgumentException ignored) {
                // Fall through for legacy unescaped file paths.
            }
        }
        if (cleaned.startsWith("file:///")) {
            cleaned = cleaned.substring("file:///".length());
        } else if (cleaned.startsWith("file://")) {
            cleaned = cleaned.substring("file://".length());
        } else if (cleaned.startsWith("file:")) {
            cleaned = cleaned.substring("file:".length());
        }
        cleaned = cleaned.replace('/', File.separatorChar);
        try {
            return Path.of(cleaned).toAbsolutePath().normalize();
        } catch (RuntimeException error) {
            throw new IOException("Invalid local Spider JAR path: " + source, error);
        }
    }

    private record JarReference(String source, String md5) {}

    private static String stringArg(JSONArray args, int index) {
        return stringArg(args, index, "");
    }

    private static String stringArg(JSONArray args, int index, String fallback) {
        if (args == null || args.isNull(index)) {
            return fallback;
        }
        return String.valueOf(args.opt(index));
    }

    private static boolean boolArg(JSONArray args, int index, boolean fallback) {
        if (args == null || args.isNull(index)) {
            return fallback;
        }
        return args.optBoolean(index, fallback);
    }

    private static HashMap<String, String> mapArg(JSONArray args, int index) {
        HashMap<String, String> map = new LinkedHashMap<>();
        if (args == null || args.isNull(index)) {
            return map;
        }
        Object value = args.opt(index);
        if (value instanceof JSONObject object) {
            for (String key : object.keySet()) {
                map.put(key, object.optString(key, ""));
            }
        }
        return map;
    }

    private static List<String> stringListArg(JSONArray args, int index) {
        List<String> values = new ArrayList<>();
        if (args == null || args.isNull(index)) {
            return values;
        }
        Object value = args.opt(index);
        if (value instanceof JSONArray array) {
            for (int i = 0; i < array.length(); i++) {
                values.add(String.valueOf(array.opt(i)));
            }
            return values;
        }
        if (value instanceof String text && !text.isEmpty()) {
            values.add(text);
        }
        return values;
    }

    private static String rootMessage(Throwable error) {
        Throwable current = error;
        while (current.getCause() != null && current.getCause() != current) {
            current = current.getCause();
        }
        String message = current.getMessage();
        return current.getClass().getSimpleName() + (message == null ? "" : (": " + message));
    }

    private static void FilesMkdirs(Path path) throws Exception {
        java.nio.file.Files.createDirectories(path);
    }
}
