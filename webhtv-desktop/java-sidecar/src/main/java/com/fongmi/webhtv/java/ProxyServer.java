package com.fongmi.webhtv.java;

import com.sun.net.httpserver.Headers;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.URLDecoder;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executors;

final class ProxyServer implements AutoCloseable {
    interface Handler {
        Object[] proxy(Map<String, String> params) throws Exception;
    }

    private static final long CACHE_TTL_MS = 5 * 60 * 1000;
    private static final int CACHE_MAX_BYTES = 512 * 1024;
    private static final int CACHE_MAX_ENTRIES = 256;
    private static final int LOG_URL_MAX_CHARS = 140;

    private static final class CachedResponse {
        final byte[] body;
        final String contentType;
        final long expiresAt;

        CachedResponse(byte[] body, String contentType, long expiresAt) {
            this.body = body;
            this.contentType = contentType;
            this.expiresAt = expiresAt;
        }
    }

    private final HttpServer server;
    private final Handler handler;
    private final ConcurrentHashMap<String, CachedResponse> cache = new ConcurrentHashMap<>();

    ProxyServer(Handler handler) throws IOException {
        this.handler = handler;
        int port = 9978;
        try {
            port = Integer.parseInt(System.getProperty("webhtv.proxy.port", "9978"));
        } catch (NumberFormatException ignored) {
            // fall back to the default port when the override is malformed
        }
        this.server = HttpServer.create(new InetSocketAddress("127.0.0.1", port), 32);
        this.server.createContext("/proxy", this::handle);
        this.server.setExecutor(Executors.newCachedThreadPool(runnable -> {
            Thread thread = new Thread(runnable, "webhtv-java-proxy");
            thread.setDaemon(true);
            return thread;
        }));
        this.server.start();
        System.err.println("java proxy listening on " + this.server.getAddress());
    }

    private void handle(HttpExchange exchange) {
        InputStream body = null;
        try {
            Map<String, String> params = query(exchange.getRequestURI().getRawQuery());
            params.putIfAbsent("method", exchange.getRequestMethod());
            for (Map.Entry<String, java.util.List<String>> entry : exchange.getRequestHeaders().entrySet()) {
                if (!entry.getValue().isEmpty()) {
                    params.putIfAbsent(entry.getKey(), entry.getValue().get(0));
                }
            }
            if ("baidu".equalsIgnoreCase(params.get("site"))) {
                params.put("User-Agent",
                        "netdisk;12.11.9;V2238A;android-android;12;JSbridge4.4.0;jointBridge;1.1.0;");
            }
            long started = System.nanoTime();
            String method = params.get("method");
            String url = params.get("url");
            System.err.println("proxy params site=" + params.get("site") + " shareId="
                    + params.get("shareId") + " fileId=" + params.get("fileId")
                    + " fileToken=" + (params.get("fileToken") == null ? "-" : "set")
                    + " ua=" + (params.get("User-Agent") == null ? "-" : "set")
                    + " cookie=" + (params.get("Cookie") == null ? "-" : "set")
                    + " range=" + (params.get("Range") == null ? "-" : params.get("Range")));
            boolean clientRanged = exchange.getRequestHeaders().getFirst("Range") != null;
            boolean cacheable = !clientRanged && cacheable(method, url);
            boolean hit = false;
            if (cacheable) {
                String cacheKey = buildCacheKey(url, params);
                CachedResponse cached = cache.get(cacheKey);
                if (cached != null && cached.expiresAt > System.currentTimeMillis()) {
                    hit = true;
                    sendBytes(exchange, 200, cached.body, cached.contentType);
                    log(method, url, 200, started, true);
                    return;
                }
                if (cached != null) cache.remove(cacheKey);
            }
            Object[] result = handler.proxy(params);
            if (result == null || result.length < 3 || !(result[0] instanceof Number)) {
                log(method, url, 404, started, false);
                sendText(exchange, 404, "proxy route is unavailable");
                return;
            }
            int status = ((Number) result[0]).intValue();
            String contentType = result[1] == null ? "application/octet-stream" : result[1].toString();
            body = result[2] instanceof InputStream ? (InputStream) result[2] : null;
            if (body == null) {
                log(method, url, status, started, false);
                sendText(exchange, status, result[2] == null ? "" : result[2].toString());
                return;
            }
            if (cacheable && status == 200
                    && !contentType.startsWith("video/")
                    && !contentType.startsWith("audio/")) {
                String cacheKey = buildCacheKey(url, params);
                ByteArrayOutputStream buffer = new ByteArrayOutputStream(8192);
                byte[] chunk = new byte[8192];
                int total = 0;
                int read;
                boolean complete = true;
                while ((read = body.read(chunk)) >= 0) {
                    if (read == 0) continue;
                    total += read;
                    if (total > CACHE_MAX_BYTES) {
                        complete = false;
                        // keep the overflow chunk: trim it and continue streaming the rest below
                        buffer.write(chunk, 0, read);
                        break;
                    }
                    buffer.write(chunk, 0, read);
                }
                if (complete) {
                    byte[] bytes = buffer.toByteArray();
                    cache.put(cacheKey, new CachedResponse(bytes, contentType, System.currentTimeMillis() + CACHE_TTL_MS));
                    pruneCache();
                    log(method, url, status, started, false);
                    sendBytes(exchange, status, bytes, contentType);
                    return;
                }
                // Over the cache limit: send headers now and stream prefix + remainder.
                exchange.getResponseHeaders().set("Content-Type", contentType);
                exchange.sendResponseHeaders(status, 0);
                try (OutputStream output = exchange.getResponseBody()) {
                    buffer.writeTo(output);
                    while ((read = body.read(chunk)) >= 0) {
                        if (read > 0) output.write(chunk, 0, read);
                    }
                }
                log(method, url, status, started, false);
                body.close();
                body = null;
                exchange.close();
                return;
            }
            if (result.length > 3 && result[3] instanceof Map<?, ?> headers) {
                copyHeaders(exchange.getResponseHeaders(), headers);
            }
            Headers responseHeaders = exchange.getResponseHeaders();
            if (!containsHeader(responseHeaders, "Content-Type")) {
                responseHeaders.set("Content-Type", contentType);
            }
            boolean head = "HEAD".equalsIgnoreCase(exchange.getRequestMethod());
            boolean lacksRangeInfo = !containsHeader(responseHeaders, "Content-Range")
                    && contentLength(responseHeaders) == 0;
            if (status == 206 && clientRanged && lacksRangeInfo) {
                responseHeaders.entrySet().removeIf(entry ->
                        entry.getKey() != null && entry.getKey().equalsIgnoreCase("Content-Range"));
                status = 200;
            }
            long length = contentLength(responseHeaders);
            exchange.sendResponseHeaders(status, head ? -1 : length);
            if (!head) {
                try (OutputStream output = exchange.getResponseBody()) {
                    byte[] buffer = new byte[64 * 1024];
                    int read;
                    while ((read = body.read(buffer)) >= 0) {
                        if (read > 0) output.write(buffer, 0, read);
                    }
                }
            }
            log(method, url, status, started, false);
        } catch (Throwable error) {
            try {
                sendText(exchange, 502, error.getMessage() == null ? error.toString() : error.getMessage());
            } catch (IOException ignored) {
            }
        } finally {
            if (body != null) {
                try {
                    body.close();
                } catch (IOException ignored) {
                }
            }
            exchange.close();
        }
    }

    private static boolean cacheable(String method, String url) {
        if (!"GET".equalsIgnoreCase(method)) return false;
        if (url == null || url.isEmpty()) return false;
        String lower = url.toLowerCase();
        if (lower.contains(".m3u8") || lower.contains(".mp4") || lower.contains(".flv")
                || lower.contains(".m4s") || lower.contains(".ts?") || lower.endsWith(".ts")
                || lower.contains(".m4a") || lower.contains(".mkv") || lower.contains(".webm")) {
            return false;
        }
        return true;
    }

    private void pruneCache() {
        if (cache.size() < CACHE_MAX_ENTRIES) return;
        long now = System.currentTimeMillis();
        cache.entrySet().removeIf(entry -> entry.getValue().expiresAt <= now);
        if (cache.size() >= CACHE_MAX_ENTRIES) {
            cache.entrySet().removeIf(entry -> entry.getValue().expiresAt <= now + CACHE_TTL_MS / 2);
        }
    }

    private static void sendBytes(HttpExchange exchange, int status, byte[] bytes, String contentType)
            throws IOException {
        Headers headers = exchange.getResponseHeaders();
        if (!containsHeader(headers, "Content-Type")) {
            headers.set("Content-Type", contentType);
        }
        exchange.sendResponseHeaders(status, bytes.length);
        try (OutputStream output = exchange.getResponseBody()) {
            output.write(bytes);
        }
    }

    private static void log(String method, String url, int status, long started, boolean hit) {
        long millis = (System.nanoTime() - started) / 1_000_000;
        String shortUrl = url == null ? "-" : url;
        if (shortUrl.length() > LOG_URL_MAX_CHARS) {
            shortUrl = shortUrl.substring(0, LOG_URL_MAX_CHARS) + "...";
        }
        System.err.println("proxy " + method + " " + shortUrl + " status=" + status
                + " took " + millis + "ms" + (hit ? " [cached]" : ""));
    }

    private static Map<String, String> query(String rawQuery) {
        Map<String, String> result = new LinkedHashMap<>();
        if (rawQuery == null || rawQuery.isEmpty()) return result;
        for (String part : rawQuery.split("&")) {
            if (part.isEmpty()) continue;
            int separator = part.indexOf('=');
            String key = separator < 0 ? part : part.substring(0, separator);
            String value = separator < 0 ? "" : part.substring(separator + 1);
            result.put(decode(key), decode(value));
        }
        return result;
    }

    private static String decode(String value) {
        return URLDecoder.decode(value, StandardCharsets.UTF_8);
    }

    private static void copyHeaders(Headers target, Map<?, ?> source) {
        for (Map.Entry<?, ?> entry : source.entrySet()) {
            if (entry.getKey() == null || entry.getValue() == null) continue;
            target.set(entry.getKey().toString(), entry.getValue().toString());
        }
    }

    private static long contentLength(Headers headers) {
        String value = headers.getFirst("Content-Length");
        if (value == null) return 0;
        try {
            return Long.parseLong(value);
        } catch (NumberFormatException ignored) {
            return 0;
        }
    }

    private static boolean containsHeader(Headers headers, String name) {
        return headers.keySet().stream().anyMatch(key -> key.equalsIgnoreCase(name));
    }

    private static String buildCacheKey(String url, Map<String, String> params) {
        StringBuilder key = new StringBuilder(url == null ? "" : url);
        String cookie = params.get("Cookie");
        if (cookie != null && !cookie.isEmpty()) {
            key.append('|').append(cookie);
        }
        String auth = params.get("Authorization");
        if (auth != null && !auth.isEmpty()) {
            key.append('|').append(auth);
        }
        return key.toString();
    }

    private static void sendText(HttpExchange exchange, int status, String text) throws IOException {
        byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", "text/plain; charset=utf-8");
        exchange.sendResponseHeaders(status, bytes.length);
        try (OutputStream output = exchange.getResponseBody()) {
            output.write(bytes);
        }
    }

    @Override
    public void close() {
        server.stop(0);
    }
}
