package com.fongmi.webhtv.java;

import com.github.catvod.crawler.Spider;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.RejectedExecutionException;
import org.json.JSONArray;
import org.json.JSONObject;

public final class SidecarMain {
    private static final Map<String, JarRuntime> RUNTIMES = new ConcurrentHashMap<>();
    private static final ExecutorService EXECUTOR = Executors.newFixedThreadPool(8, runnable -> {
        Thread thread = new Thread(runnable, "webhtv-java-request");
        thread.setDaemon(true);
        return thread;
    });

    public static void main(String[] args) throws Exception {
        System.setOut(new java.io.PrintStream(System.out, true, StandardCharsets.UTF_8));
        System.setErr(new java.io.PrintStream(System.err, true, StandardCharsets.UTF_8));
        String configured = System.getProperty("webhtv.java.workDir");
        Path workDir = Path.of(
                        configured == null || configured.isBlank()
                                ? Path.of(System.getProperty("java.io.tmpdir"), "webhtv-java-sidecar").toString()
                                : configured)
                .toAbsolutePath()
                .normalize();
        java.nio.file.Files.createDirectories(workDir);
        System.setErr(new java.io.PrintStream(new TeeStream(System.err, workDir.resolve("sidecar-stderr.log")), true, StandardCharsets.UTF_8));
        System.err.println("webhtv-java-sidecar ready workDir=" + workDir);
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                if (line.isBlank()) {
                    continue;
                }
                JSONObject request;
                try {
                    request = new JSONObject(line);
                } catch (Exception error) {
                    writeError(null, "invalid JSON request: " + error.getMessage());
                    continue;
                }
                String id = request.optString("id", null);
                String type = request.optString("type", "");
                if ("ping".equals(type)) {
                    writeOk(id, encodeResult(new JSONObject().put("pong", true)));
                    continue;
                }
                if ("shutdown".equals(type)) {
                    writeOk(id, new JSONObject().put("shutdown", true));
                    EXECUTOR.shutdownNow();
                    closeAll();
                    return;
                }
                String siteKey = request.optString("siteKey", "site");
                long started = System.nanoTime();
                try {
                    EXECUTOR.execute(() -> {
                        try {
                            switch (type) {
                                case "invoke" -> handleInvoke(id, request, workDir);
                                case "config" -> handleConfig(id, request, workDir);
                                case "auth" -> handleAuth(id, request, workDir);
                                case "parse" -> handleParse(id, request, workDir);
                                default -> writeError(id, "unsupported request type `" + type + "`");
                            }
                        } catch (Throwable error) {
                            writeError(id, rootMessage(error));
                        } finally {
                            long elapsedMs = (System.nanoTime() - started) / 1_000_000;
                            String label = request.optString("api", "");
                            String method = type.equals("invoke") ? request.optString("method", "") : "";
                            String entry = java.time.LocalTime.now() + " sidecar " + type + " " + method + " " + label
                                    + " took " + elapsedMs + " ms (id=" + id + ")";
                            if ("auth".equals(type)) {
                                entry += " operation=" + request.optString("operation", "")
                                        + " provider=" + request.optString("provider", "")
                                        + " sessionId=" + request.optString("sessionId", "");
                            }
                            System.err.println(entry);
                            appendLog(workDir, entry);
                        }
                    });
                } catch (RejectedExecutionException error) {
                    writeError(id, "sidecar is shutting down");
                }
            }
        } finally {
            closeAll();
        }
    }

    private static void handleInvoke(String id, JSONObject request, Path workDir) throws Exception {
        String siteKey = request.optString("siteKey", "site");
        String api = request.optString("api", "");
        String jar = request.optString("jar", "");
        String ext = request.has("ext") && !request.isNull("ext")
                ? (request.opt("ext") instanceof String
                        ? request.optString("ext")
                        : String.valueOf(request.opt("ext")))
                : "";
        String method = request.optString("method", "");
        JSONArray args = request.optJSONArray("args");
        if (args == null) {
            args = new JSONArray();
        }
        JarRuntime runtime = RUNTIMES.computeIfAbsent("default", key -> {
            try {
                return new JarRuntime(workDir);
            } catch (Exception error) {
                throw new RuntimeException(error);
            }
        });
        Spider spider = runtime.ensureSpider(siteKey, api, ext, jar);
        Object result = runtime.invoke(spider, method, args);
        writeOk(id, encodeResult(result));
    }

    private static void handleConfig(String id, JSONObject request, Path workDir) throws Exception {
        JarRuntime runtime = RUNTIMES.computeIfAbsent("default", key -> {
            try {
                return new JarRuntime(workDir);
            } catch (Exception error) {
                throw new RuntimeException(error);
            }
        });
        JSONArray args = request.optJSONArray("args");
        String key = request.optString("key", args == null ? "" : args.optString(0, ""));
        String value = request.optString("value", args == null ? "" : args.optString(1, ""));
        if ("get".equals(request.optString("operation", "set"))) {
            writeOk(id, encodeResult(runtime.getConfig(key)));
        } else {
            runtime.setConfig(key, value);
            writeOk(id, encodeResult("{}"));
        }
    }

    private static void handleAuth(String id, JSONObject request, Path workDir) throws Exception {
        JarRuntime runtime = RUNTIMES.computeIfAbsent("default", key -> {
            try {
                return new JarRuntime(workDir);
            } catch (Exception error) {
                throw new RuntimeException(error);
            }
        });
        String siteKey = request.optString("siteKey", "config");
        String api = request.optString("api", "");
        String jar = request.optString("jar", "");
        String ext = request.has("ext") && !request.isNull("ext")
                ? (request.opt("ext") instanceof String
                        ? request.optString("ext")
                        : String.valueOf(request.opt("ext")))
                : "";
        runtime.ensureSpider(siteKey, api, ext, jar);
        String result = runtime.cloudAuth(
                request.optString("operation", ""),
                request.optString("provider", ""),
                request.optString("sessionId", ""));
        writeOk(id, encodeResult(result));
    }

    private static void handleParse(String id, JSONObject request, Path workDir) throws Exception {
        String siteKey = request.optString("siteKey", "site");
        String api = request.optString("api", "");
        String jar = request.optString("jar", "");
        String ext = request.has("ext") && !request.isNull("ext")
                ? (request.opt("ext") instanceof String
                        ? request.optString("ext")
                        : String.valueOf(request.opt("ext")))
                : "";
        JarRuntime runtime = RUNTIMES.computeIfAbsent("default", key -> {
            try {
                return new JarRuntime(workDir);
            } catch (Exception error) {
                throw new RuntimeException(error);
            }
        });
        runtime.ensureSpider(siteKey, api, ext, jar);
        Object result = runtime.parse(
                request.optInt("parserType", 0),
                request.optString("parserKey", ""),
                request.optString("parserName", ""),
                request.optString("flag", ""),
                request.optString("url", ""),
                request.optJSONObject("parsers"));
        writeOk(id, encodeResult(result));
    }

    private static JSONObject encodeResult(Object result) {
        JSONObject payload = new JSONObject();
        if (result == null) {
            return payload.put("kind", "undefined");
        }
        if (result instanceof Boolean bool) {
            return payload.put("kind", "value").put("value", bool);
        }
        if (result instanceof Number number) {
            return payload.put("kind", "value").put("value", number);
        }
        if (result instanceof String text) {
            return payload.put("kind", "value").put("value", text);
        }
        return payload.put("kind", "value").put("value", String.valueOf(result));
    }

    private static void writeOk(String id, JSONObject data) {
        JSONObject response = new JSONObject();
        if (id != null) {
            response.put("id", id);
        }
        response.put("ok", true);
        response.put("data", data);
        System.out.println(response);
        System.out.flush();
    }

    private static void writeError(String id, String message) {
        JSONObject response = new JSONObject();
        if (id != null) {
            response.put("id", id);
        }
        response.put("ok", false);
        response.put("error", message == null ? "unknown error" : message);
        System.out.println(response);
        System.out.flush();
    }

    private static void closeAll() {
        for (JarRuntime runtime : RUNTIMES.values()) {
            runtime.close();
        }
        RUNTIMES.clear();
    }

    private static void appendLog(Path workDir, String entry) {
        try {
            java.nio.file.Files.writeString(
                    workDir.resolve("requests.log"),
                    entry + System.lineSeparator(),
                    java.nio.charset.StandardCharsets.UTF_8,
                    java.nio.file.StandardOpenOption.CREATE,
                    java.nio.file.StandardOpenOption.APPEND);
        } catch (Exception error) {
            System.err.println("request log write failed: " + error.getMessage());
        }
    }

    private static String rootMessage(Throwable error) {
        Throwable current = error;
        while (current.getCause() != null) {
            current = current.getCause();
        }
        String message = current.getMessage();
        return current.getClass().getSimpleName() + (message == null ? "" : (": " + message));
    }

    private static final class TeeStream extends java.io.OutputStream {
        private final java.io.OutputStream primary;
        private final Path logFile;

        TeeStream(java.io.OutputStream primary, Path logFile) {
            this.primary = primary;
            this.logFile = logFile;
        }

        @Override
        public void write(int value) throws java.io.IOException {
            primary.write(value);
            appendToFile(new byte[] {(byte) value}, 0, 1);
        }

        @Override
        public void write(byte[] buffer, int offset, int length) throws java.io.IOException {
            primary.write(buffer, offset, length);
            appendToFile(buffer, offset, length);
        }

        @Override
        public void flush() throws java.io.IOException {
            primary.flush();
        }

        private void appendToFile(byte[] buffer, int offset, int length) {
            try {
                java.nio.file.Files.write(
                        logFile,
                        java.util.Arrays.copyOfRange(buffer, offset, offset + length),
                        java.nio.file.StandardOpenOption.CREATE,
                        java.nio.file.StandardOpenOption.APPEND);
            } catch (Exception ignored) {
                // Diagnostics must never break the sidecar protocol.
            }
        }
    }
}
