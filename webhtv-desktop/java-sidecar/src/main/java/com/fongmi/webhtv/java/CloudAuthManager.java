package com.fongmi.webhtv.java;

import android.graphics.Bitmap;
import java.io.IOException;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.net.SocketTimeoutException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.SecureRandom;
import java.time.Duration;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import okhttp3.FormBody;
import okhttp3.Headers;
import okhttp3.OkHttpClient;
import okhttp3.Request;
import okhttp3.Response;
import okhttp3.ResponseBody;
import org.json.JSONObject;

final class CloudAuthManager {
    private static final Duration SESSION_TTL = Duration.ofMinutes(10);
    private static final int MAX_POLL_FAILURES = 5;
    private static final String QUARK_USER_AGENT =
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) "
                    + "quark-cloud-drive/3.0.1 Chrome/100.0.4896.160 Electron/18.3.5.12-a038f7b798 "
                    + "Safari/537.36 Channel/pckk_other_ch";
    private static final String UC_USER_AGENT =
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) "
                    + "Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0";
    private static final String BAIDU_USER_AGENT =
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) "
                    + "Chrome/109.0.0.0 Safari/537.36";
    private static final String UC_TV_USER_AGENT =
            "Mozilla/5.0 (Linux; Android 12; V2238A Build/SP1A.210812.003; wv) AppleWebKit/537.36 "
                    + "(KHTML, like Gecko) Version/4.0 Chrome/102.0.5005.125 Mobile Safari/533.1";
    private static final String UC_TV_CLIENT_ID = "5acf882d27b74502b7040b0c65519aa7";
    private static final String UC_TV_SECRET = "l3srvtd7p42l0d0x1u8d7yc8ye9kki4d";
    private static final String UC_TV_DEVICE_MODEL = "V2238A";
    private static final String UC_TV_EXCHANGE_URL = "http://api.extscreen.com/ucdrive/token";

    private final Path credentialDir;
    private final SecureRandom random = new SecureRandom();
    private final Map<String, Session> sessions = new ConcurrentHashMap<>();
    private final OkHttpClient client = new OkHttpClient.Builder()
            .connectTimeout(Duration.ofSeconds(10))
            .readTimeout(Duration.ofSeconds(25))
            .callTimeout(Duration.ofSeconds(30))
            .followRedirects(true)
            .followSslRedirects(true)
            .build();
    private final OkHttpClient shortPollClient = client.newBuilder()
            .readTimeout(Duration.ofSeconds(8))
            .callTimeout(Duration.ofSeconds(12))
            .build();
    private final OkHttpClient probeClient = client.newBuilder()
            .readTimeout(Duration.ofSeconds(2))
            .callTimeout(Duration.ofSeconds(3))
            .build();

    CloudAuthManager(Path workDir) throws IOException {
        credentialDir = workDir.resolve("external-storage").resolve("TVBox");
        Files.createDirectories(credentialDir);
    }

    synchronized JSONObject handle(
            String operation, String provider, String sessionId, ClassLoader jarLoader) throws Exception {
        return switch (operation) {
            case "start" -> start(normalizeProvider(provider), jarLoader);
            case "poll" -> poll(sessionId);
            case "cancel" -> cancel(sessionId);
            case "clear" -> clear(normalizeProvider(provider));
            case "status" -> status(normalizeProvider(provider));
            default -> throw new IllegalArgumentException("unsupported cloud auth operation `" + operation + "`");
        };
    }

    private JSONObject start(String provider, ClassLoader jarLoader) throws Exception {
        if (jarLoader == null) throw new IllegalStateException("cloud login requires a loaded Spider JAR");
        sessions.values().removeIf(session -> session.provider.equals(provider));
        Session session = switch (provider) {
            case "quark" -> startQuark(jarLoader);
            case "uc" -> startUc(jarLoader);
            case "uctv" -> startUcTv(jarLoader);
            case "baidu" -> startBaidu(jarLoader);
            default -> throw new IllegalArgumentException("unsupported cloud provider `" + provider + "`");
        };
        sessions.put(session.id, session);
        JSONObject result = state(session, "pending", "等待扫码");
        if (!session.qrText.isEmpty()) result.put("qrText", session.qrText);
        if (!session.qrImage.isEmpty()) result.put("qrImage", session.qrImage);
        if ("baidu".equals(provider)) result.put("sign", session.token);
        return result;
    }

    private Session startQuark(ClassLoader jarLoader) throws Exception {
        Headers headers = new Headers.Builder().add("User-Agent", QUARK_USER_AGENT).build();
        JSONObject response = getJson(
                client,
                "https://uop.quark.cn/cas/ajax/getTokenForQrcodeLogin?client_id=532&v=1.2",
                headers);
        String token = membersToken(response);
        String qrText = "https://su.quark.cn/4_eMHBJ?token=" + token
                + "&client_id=532&ssb=weblogin&uc_param_str=&uc_biz_str="
                + "S%3Acustom%7COPT%3ASAREA%400%7COPT%3AIMMERSIVE%401%7COPT%3ABACK_BTN_STYLE%400";
        return new Session(newId(), "quark", token, qrText, "", jarLoader, null);
    }

    private Session startUc(ClassLoader jarLoader) throws Exception {
        long requestId = System.currentTimeMillis();
        Headers headers = new Headers.Builder()
                .add("Accept", "application/json, text/plain, */*")
                .add("User-Agent", UC_USER_AGENT)
                .add("Referer", "https://broccoli.uc.cn/")
                .build();
        JSONObject response = getJson(
                client,
                "https://api.open.uc.cn/cas/ajax/getTokenForQrcodeLogin?pr=UCBrowser&fr=pc&sys=win32"
                        + "&client_id=529&v=1.2&request_id=" + requestId,
                headers);
        String token = membersToken(response);
        String qrText = "https://su.uc.cn/4_fzMj2?uc_param_str="
                + "dsdnfrpfbivesscpgimibtbmnijblauputogpintnwktprchmt&token=" + token
                + "&client_id=529&uc_biz_str=S%3Acustom%7CC%3Atitlebar_fix";
        return new Session(newId(), "uc", token, qrText, "", jarLoader, null);
    }

    private Session startUcTv(ClassLoader jarLoader) throws Exception {
        String now = String.valueOf(System.currentTimeMillis());
        String deviceId = md5Hex(now);
        String reqId = md5Hex(md5Hex(now) + now);
        String token = signPan("GET&/oauth/authorize&" + now + "&" + UC_TV_SECRET);
        Headers headers = ucTvHeaders(token, now, false);
        JSONObject response = getJson(
                client,
                "https://open-api-drive.uc.cn/oauth/authorize"
                        + "?req_id=" + reqId
                        + "&access_token=&app_ver=1.6.8"
                        + "&device_id=" + deviceId
                        + "&device_brand=vivo&platform=tv"
                        + "&device_name=" + UC_TV_DEVICE_MODEL
                        + "&device_model=" + UC_TV_DEVICE_MODEL
                        + "&build_device=" + UC_TV_DEVICE_MODEL
                        + "&build_product=" + UC_TV_DEVICE_MODEL
                        + "&device_gpu=Adreno%20(TM)%20640"
                        + "&activity_rect=%7B%7D"
                        + "&channel=UCTVOFFICIALWEB"
                        + "&auth_type=code"
                        + "&client_id=" + UC_TV_CLIENT_ID
                        + "&scope=netdisk"
                        + "&qrcode=1&qr_width=460&qr_height=460",
                headers);
        int status = response.optInt("status", -1);
        String queryToken = response.optString("query_token", "").trim();
        String qrData = response.optString("qr_data", "").trim();
        System.err.println("uc tv authorize status=" + status + " queryTokenLen=" + queryToken.length()
                + " qrLen=" + qrData.length());
        if (status != 0 || queryToken.isEmpty() || qrData.isEmpty()) {
            throw new IOException("UC TV 未返回有效二维码 status=" + status);
        }
        String qrImage = "data:image/png;base64," + qrData;
        return new Session(newId(), "uctv", queryToken, "", qrImage, jarLoader, null, deviceId);
    }

    private Headers ucTvHeaders(String panToken, String panTm, boolean withHost) {
        Headers.Builder builder = new Headers.Builder()
                .add("User-Agent", UC_TV_USER_AGENT)
                .add("x-pan-tm", panTm)
                .add("x-pan-token", panToken)
                .add("content-type", "text/plain;charset=UTF-8")
                .add("x-pan-client-id", UC_TV_CLIENT_ID);
        if (withHost) builder.add("host", "open-api-drive.uc.cn");
        return builder.build();
    }

    private static String md5Hex(String value) throws Exception {
        return hexDigest("MD5", value);
    }

    private static String signPan(String value) throws Exception {
        return hexDigest("SHA-256", value);
    }

    private static String hexDigest(String algorithm, String value) throws Exception {
        java.security.MessageDigest digest = java.security.MessageDigest.getInstance(algorithm);
        byte[] bytes = digest.digest(value.getBytes(StandardCharsets.UTF_8));
        StringBuilder hex = new StringBuilder(bytes.length * 2);
        for (byte b : bytes) {
            String part = Integer.toHexString(b & 0xff);
            if (part.length() < 2) hex.append('0');
            hex.append(part);
        }
        return hex.toString();
    }

    private Session startBaidu(ClassLoader jarLoader) throws Exception {
        Class<?> baiduType = findClass(
                jarLoader,
                "com.github.catvod.spider.merge.b.j",
                "com.github.catvod.spider.merge.B.j");
        String sign = "";
        byte[] image = new byte[0];
        String lastError = "百度未返回有效二维码";
        for (int attempt = 1; attempt <= 3; attempt++) {
            boolean created = (Boolean) baiduType.getMethod("o").invoke(null);
            Field signField = baiduType.getDeclaredField("e");
            signField.setAccessible(true);
            sign = String.valueOf(signField.get(null)).trim();
            Object instance = baiduType.getMethod("f").invoke(null);
            Bitmap bitmap = (Bitmap) baiduType.getMethod("n").invoke(instance);
            image = bitmap == null ? new byte[0] : bitmap.getData();
            if (sign.isEmpty() || image.length == 0) {
                lastError = "百度未返回有效二维码";
                System.err.println("baidu start attempt=" + attempt + " failed: sign=" + sign.length()
                        + " img=" + image.length);
                continue;
            }
            if (probeBaiduChannel(sign)) break;
            lastError = "百度二维码 channel 未就绪，自动重试 " + attempt;
            System.err.println(lastError + " sign=" + sign);
            Thread.sleep(500);
        }
        if (sign.isEmpty() || image.length == 0) throw new IOException(lastError);
        String dataUrl = "data:image/png;base64," + Base64.getEncoder().encodeToString(image);
        return new Session(newId(), "baidu", sign, "", dataUrl, jarLoader, baiduType);
    }

    private boolean probeBaiduChannel(String channelId) {
        Headers headers = new Headers.Builder()
                .add("User-Agent", BAIDU_USER_AGENT)
                .add("Referer", "https://pan.baidu.com/")
                .build();
        try {
            JSONObject channel = getJson(
                    probeClient,
                    "https://passport.baidu.com/channel/unicast?channel_id=" + channelId,
                    headers);
            int errno = channel.optInt("errno", -1);
            String status = channel.optString("channel_v", "");
            System.err.println("baidu probe channel_id=" + channelId + " errno=" + errno
                    + " vLen=" + status.length());
            return errno == 0;
        } catch (SocketTimeoutException error) {
            System.err.println("baidu probe channel_id=" + channelId + " ok (waiting)");
            return true;
        } catch (Exception error) {
            System.err.println("baidu probe channel_id=" + channelId + " failed: " + safeError(error));
            return false;
        }
    }

    private JSONObject poll(String sessionId) throws Exception {
        Session session = sessions.get(sessionId == null ? "" : sessionId.trim());
        if (session == null) {
            return new JSONObject().put("state", "expired").put("message", "登录会话不存在或已结束");
        }
        if (System.currentTimeMillis() >= session.expiresAt) {
            sessions.remove(session.id);
            return state(session, "expired", "二维码已过期，请重新生成");
        }
        try {
            JSONObject result = switch (session.provider) {
                case "quark" -> pollQuark(session);
                case "uc" -> pollUc(session);
                case "uctv" -> pollUcTv(session);
                case "baidu" -> pollBaidu(session);
                default -> throw new IllegalStateException("unsupported cloud provider");
            };
            if ("success".equals(result.optString("state"))) sessions.remove(session.id);
            session.failures = 0;
            return result;
        } catch (SocketTimeoutException error) {
            return state(session, "pending", "等待扫码确认");
        } catch (Exception error) {
            Throwable root = rootCause(error);
            System.err.println("cloud login poll failed provider=" + session.provider + " error="
                    + root.getClass().getSimpleName() + ": " + safeError(root));
            session.failures += 1;
            String message = failureMessage(session.provider, root);
            if (session.failures >= MAX_POLL_FAILURES) {
                sessions.remove(session.id);
                return state(session, "error", message);
            }
            return state(session, "pending", message + "，正在重试");
        }
    }

    private JSONObject pollQuark(Session session) throws Exception {
        JSONObject response = getJson(
                client,
                "https://uop.quark.cn/cas/ajax/getServiceTicketByQrcodeToken?client_id=532&v=1.2&token="
                        + session.token,
                Headers.of());
        System.err.println("quark ticket poll message=" + response.optString("message")
                + " data=" + response.optJSONObject("data"));
        if (!"ok".equalsIgnoreCase(response.optString("message"))) {
            return state(session, "pending", "请使用夸克浏览器扫码并确认");
        }
        String ticket = response.optJSONObject("data")
                .optJSONObject("members")
                .optString("service_ticket", "")
                .trim();
        if (ticket.isEmpty()) return state(session, "pending", "等待夸克确认登录");
        System.err.println("quark ticket acquired ticketLen=" + ticket.length());

        Headers headers = new Headers.Builder()
                .add("User-Agent", "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 "
                        + "(KHTML, like Gecko) Chrome/38.0.2125.122 Safari/537.36")
                .add("Accept", "application/json, text/plain, */*")
                .add("Referer", "https://pan.quark.cn/")
                .build();
        List<String> setCookies = responseHeaders(
                client, "https://pan.quark.cn/account/info?st=" + ticket + "&lw=scan", headers, "Set-Cookie");
        System.err.println("quark tickets response cookies=" + setCookies.size() + " tk=" + response.optString("message"));
        String cookie = cookieHeader(setCookies, "__pus=");
        System.err.println("quark cookie after account swap len=" + cookie.length());
        if (cookie.isEmpty()) throw new IOException("夸克登录成功，但未返回有效凭据");
        String account = saveWithJar(session, cookie);
        return state(session, "success", "夸克登录成功").put("account", account);
    }

    private JSONObject pollUc(Session session) throws Exception {
        long requestId = System.currentTimeMillis();
        FormBody body = new FormBody.Builder()
                .add("client_id", "381")
                .add("v", "1.2")
                .add("request_id", String.valueOf(requestId))
                .add("token", session.token)
                .build();
        Request request = new Request.Builder()
                .url("https://api.open.uc.cn/cas/ajax/getServiceTicketByQrcodeToken?__dt=18884&__t="
                        + requestId)
                .headers(new Headers.Builder()
                        .add("Accept", "application/json, text/plain, */*")
                        .add("Content-Type", "application/x-www-form-urlencoded")
                        .add("User-Agent", UC_USER_AGENT)
                        .add("Referer", "https://broccoli.uc.cn/")
                        .build())
                .post(body)
                .build();
        JSONObject response = new JSONObject(executeText(client, request));
        if (!"ok".equalsIgnoreCase(response.optString("message"))) {
            return state(session, "pending", "请使用 UC 浏览器扫码并确认");
        }
        String ticket = response.optJSONObject("data")
                .optJSONObject("members")
                .optString("service_ticket", "")
                .trim();
        if (ticket.isEmpty()) return state(session, "pending", "等待 UC 确认登录");
        Headers headers = new Headers.Builder()
                .add("Accept", "application/json, text/plain, */*")
                .add("Content-Type", "application/x-www-form-urlencoded")
                .add("User-Agent", UC_USER_AGENT)
                .add("Referer", "https://broccoli.uc.cn/")
                .build();
        List<String> setCookies = responseHeaders(
                client, "https://drive.uc.cn/account/info?st=" + ticket, headers, "Set-Cookie");
        String cookie = cookieHeader(setCookies, "");
        if (cookie.isEmpty()) throw new IOException("UC 登录成功，但未返回有效凭据");
        String account = saveWithJar(session, cookie);
        return state(session, "success", "UC 登录成功").put("account", account);
    }

    private JSONObject pollUcTv(Session session) throws Exception {
        String now = String.valueOf(System.currentTimeMillis());
        String reqId = md5Hex(session.deviceId + now);
        String token = signPan("GET&/oauth/code&" + now + "&" + UC_TV_SECRET);
        Headers headers = ucTvHeaders(token, now, true);
        JSONObject response = getJson(
                shortPollClient,
                "https://open-api-drive.uc.cn/oauth/code"
                        + "?req_id=" + reqId
                        + "&access_token=&app_ver=1.6.8"
                        + "&device_id=" + session.deviceId
                        + "&device_brand=vivo&platform=tv"
                        + "&device_name=" + UC_TV_DEVICE_MODEL
                        + "&device_model=" + UC_TV_DEVICE_MODEL
                        + "&build_device=" + UC_TV_DEVICE_MODEL
                        + "&build_product=" + UC_TV_DEVICE_MODEL
                        + "&device_gpu=Adreno%20(TM)%20640"
                        + "&activity_rect=%7B%7D"
                        + "&channel=UCTVOFFICIALWEB"
                        + "&auth_type=code"
                        + "&client_id=" + UC_TV_CLIENT_ID
                        + "&scope=netdisk"
                        + "&query_token=" + session.token,
                headers);
        int status = response.optInt("status", -1);
        int errno = response.optInt("errno", 0);
        System.err.println("uc tv poll status=" + status + " errno=" + errno
                + " resp=" + response.toString().replaceAll("https?://\\S+", "<url>"));
        if (status != 0) {
            return state(session, "pending", "请使用 UC 浏览器 APP 扫码并确认");
        }
        String code = response.optString("code", "").trim();
        if (code.isEmpty()) return state(session, "pending", "等待 UC 确认登录");
        System.err.println("uc tv code acquired codeLen=" + code.length());
        String exchangeNow = String.valueOf(System.currentTimeMillis());
        String exchangeReqId = md5Hex(session.deviceId + exchangeNow);
        JSONObject exchange = new JSONObject()
                .put("req_id", exchangeReqId)
                .put("app_ver", "1.6.8")
                .put("device_id", session.deviceId)
                .put("device_brand", "vivo")
                .put("platform", "tv")
                .put("device_name", UC_TV_DEVICE_MODEL)
                .put("device_model", UC_TV_DEVICE_MODEL)
                .put("build_device", UC_TV_DEVICE_MODEL)
                .put("build_product", UC_TV_DEVICE_MODEL)
                .put("device_gpu", "Adreno (TM) 640")
                .put("activity_rect", "{}")
                .put("channel", "UCTVOFFICIALWEB")
                .put("code", code);
        Request exchangeRequest = new Request.Builder()
                .url(UC_TV_EXCHANGE_URL)
                .headers(new Headers.Builder()
                        .add("Content-Type", "application/json")
                        .add("Accept", "application/json")
                        .build())
                .post(okhttp3.RequestBody.create(exchange.toString(), okhttp3.MediaType.get("application/json")))
                .build();
        JSONObject exchangeResponse = new JSONObject(executeText(client, exchangeRequest));
        int exchangeCode = exchangeResponse.optInt("code", -1);
        System.err.println("uc tv exchange code=" + exchangeCode
                + " resp=" + exchangeResponse.toString().replaceAll("https?://\\S+", "<url>"));
        if (exchangeCode != 200) {
            throw new IOException("UC TV 凭据交换失败 code=" + exchangeCode);
        }
        JSONObject data = exchangeResponse.optJSONObject("data");
        if (data == null || data.optString("access_token", "").isEmpty()) {
            throw new IOException("UC TV 凭据交换失败: 未返回 access_token");
        }
        JSONObject credential = new JSONObject(data.toString())
                .put("status", 0)
                .put("start_time", System.currentTimeMillis() / 1000)
                .put("device_id", session.deviceId);
        Files.writeString(credentialPath("uctv"), credential.toString(), StandardCharsets.UTF_8);
        return state(session, "success", "UC TV 登录成功");
    }

    private JSONObject pollBaidu(Session session) throws Exception {
        Headers headers = new Headers.Builder()
                .add("User-Agent", BAIDU_USER_AGENT)
                .add("Referer", "https://pan.baidu.com/")
                .build();
        JSONObject channel;
        try {
            channel = getJson(
                    shortPollClient,
                    "https://passport.baidu.com/channel/unicast?channel_id=" + session.token,
                    headers);
        } catch (SocketTimeoutException error) {
            System.err.println("baidu unicast timeout channel_id=" + session.token);
            throw error;
        }
        int errno = channel.optInt("errno", -1);
        String rawValue = channel.optString("channel_v", "");
        JSONObject value = parseJsonObject(rawValue);
        int status = value.optInt("status", -1);
        String confirmed = value.optString("v", "").trim();
        System.err.println("baidu unicast channel_id=" + session.token + " errno=" + errno
                + " status=" + status + " vLen=" + confirmed.length() + " v=" + confirmed);
        if (errno != 0) {
            return state(session, "pending", "请使用百度网盘扫码并确认");
        }
        if (status != 0) {
            return state(session, "pending", "请在百度网盘中确认登录");
        }
        if (confirmed.isBlank()) return state(session, "pending", "等待百度确认登录");
        String bduss = confirmed;
        String realBduss = "";
        String username = "";
        List<String> qrCookies = List.of();
        String lastError = "";
        for (int attempt = 1; attempt <= 2 && realBduss.isEmpty(); attempt++) {
            long ts = System.currentTimeMillis();
            String qrUrl = "https://passport.baidu.com/v3/login/main/qrbdusslogin?v=" + ts + "&bduss=" + bduss;
            Request qrRequest = new Request.Builder()
                    .url(qrUrl)
                    .headers(headers)
                    .get()
                    .build();
            String qrResp;
            try (Response response = shortPollClient.newCall(qrRequest).execute()) {
                requireSuccess(response);
                qrResp = response.body() == null ? "" : response.body().string();
                qrCookies = response.headers("Set-Cookie");
            }
            System.err.println("baidu qrbdusslogin attempt=" + attempt + " len=" + qrResp.length()
                    + " cookies=" + qrCookies.size());
            String bdussCode = extractJsonString(qrResp, "code");
            if (!"110000".equals(bdussCode) && !"310000".equals(bdussCode) && !"0".equals(bdussCode)) {
                lastError = "百度凭据交换失败 code=" + bdussCode;
                continue;
            }
            realBduss = extractJsonString(qrResp, "bduss");
            username = extractJsonString(qrResp, "username");
            if (username.isEmpty()) username = extractJsonString(qrResp, "displayName");
            if (realBduss.isEmpty()) lastError = "百度凭据交换失败: 未返回 BDUSS";
        }
        if (realBduss.isEmpty()) throw new IOException(lastError);
        StringBuilder cookieBuilder = new StringBuilder();
        for (String setCookie : qrCookies) {
            String pair = setCookie.split(";", 2)[0].trim();
            if (pair.isEmpty() || pair.indexOf('=') <= 0) continue;
            if (cookieBuilder.length() > 0) cookieBuilder.append(';');
            cookieBuilder.append(pair);
        }
        String diskCookie = cookieBuilder.toString();
        if (!diskCookie.contains("BDUSS=")) diskCookie = "BDUSS=" + realBduss;
        System.err.println("baidu cookie=" + diskCookie);
        JSONObject credential = new JSONObject()
                .put("cookie", diskCookie)
                .put("username", username);
        Files.writeString(credentialPath("baidu"), credential.toString(), StandardCharsets.UTF_8);
        String account = readAccount("baidu");
        System.err.println("baidu login completed account=" + account);
        return state(session, "success", "百度登录成功").put("account", account);
    }

    private JSONObject cancel(String sessionId) {
        Session removed = sessions.remove(sessionId == null ? "" : sessionId.trim());
        return new JSONObject()
                .put("state", "cancelled")
                .put("message", removed == null ? "登录会话已结束" : "已取消登录");
    }

    private JSONObject clear(String provider) throws IOException {
        Files.deleteIfExists(credentialPath(provider));
        sessions.values().removeIf(session -> session.provider.equals(provider));
        return new JSONObject()
                .put("provider", provider)
                .put("state", "cleared")
                .put("message", providerLabel(provider) + "登录信息已清除");
    }

    private JSONObject status(String provider) throws IOException {
        if ("uctv".equals(provider)) {
            Path path = credentialPath(provider);
            boolean loggedIn = Files.isRegularFile(path) && Files.size(path) > 2;
            JSONObject value = loggedIn
                    ? new JSONObject(Files.readString(path, StandardCharsets.UTF_8))
                    : new JSONObject();
            return new JSONObject()
                    .put("provider", provider)
                    .put("state", loggedIn ? "authenticated" : "anonymous")
                    .put("message", loggedIn ? "已登录" : "未登录")
                    .put("nickname", value.optString("nickname", ""));
        }
        Path path = credentialPath(provider);
        boolean loggedIn = Files.isRegularFile(path) && Files.size(path) > 2;
        return new JSONObject()
                .put("provider", provider)
                .put("state", loggedIn ? "authenticated" : "anonymous")
                .put("message", loggedIn ? "已登录" : "未登录");
    }

    private String saveWithJar(Session session, String cookie) throws Exception {
        boolean quark = "quark".equals(session.provider);
        Class<?> type = quark
                ? findClass(
                        session.jarLoader,
                        "com.github.catvod.spider.merge.b.w",
                        "com.github.catvod.spider.merge.B.w")
                : findClass(
                        session.jarLoader,
                        "com.github.catvod.spider.merge.b.B",
                        "com.github.catvod.spider.merge.B.B");
        Object instance = type.getMethod(quark ? "e" : "c").invoke(null);
        boolean saved = (Boolean) type.getMethod(quark ? "v" : "t", String.class).invoke(instance, cookie);
        if (!saved) throw new IOException(providerLabel(session.provider) + "登录凭据校验失败");
        return readAccount(session.provider);
    }

    private String readAccount(String provider) throws IOException {
        Path path = credentialPath(provider);
        if (!Files.isRegularFile(path) || Files.size(path) <= 2) {
            throw new IOException(providerLabel(provider) + "登录信息未写入");
        }
        JSONObject value = new JSONObject(Files.readString(path, StandardCharsets.UTF_8));
        String account = "baidu".equals(provider)
                ? value.optString("username", value.optString("displayname", ""))
                : value.optString("nickname", "");
        return account.isBlank() ? providerLabel(provider) + "账号" : account;
    }

    private Path credentialPath(String provider) {
        return credentialDir.resolve(switch (provider) {
            case "quark" -> "quark_cookie.txt";
            case "uc", "uctv" -> "uc_cookie.txt";
            case "baidu" -> "baidu.txt";
            default -> throw new IllegalArgumentException("unsupported cloud provider `" + provider + "`");
        });
    }

    private JSONObject state(Session session, String state, String message) {
        return new JSONObject()
                .put("sessionId", session.id)
                .put("provider", session.provider)
                .put("state", state)
                .put("message", message)
                .put("expiresAt", session.expiresAt);
    }

    private static JSONObject getJson(OkHttpClient client, String url, Headers headers) throws Exception {
        return parseJsonObject(getText(client, url, headers));
    }

    private static String getText(OkHttpClient client, String url, Headers headers) throws IOException {
        Request request = new Request.Builder().url(url).headers(headers).get().build();
        return executeText(client, request);
    }

    private static String executeText(OkHttpClient client, Request request) throws IOException {
        try (Response response = client.newCall(request).execute()) {
            requireSuccess(response);
            ResponseBody body = response.body();
            if (body == null) throw new IOException("empty HTTP response from " + request.url());
            return body.string();
        }
    }

    private static List<String> responseHeaders(
            OkHttpClient client, String url, Headers headers, String name) throws IOException {
        Request request = new Request.Builder().url(url).headers(headers).get().build();
        try (Response response = client.newCall(request).execute()) {
            requireSuccess(response);
            return response.headers(name);
        }
    }

    private static void requireSuccess(Response response) throws IOException {
        if (!response.isSuccessful()) {
            throw new IOException("HTTP " + response.code() + " from " + response.request().url());
        }
    }

    private static String extractJsonString(String text, String key) {
        if (text == null) return "";
        int idx = text.indexOf('"' + key + '"');
        if (idx < 0) return "";
        int colon = text.indexOf(':', idx);
        if (colon < 0) return "";
        int quote = text.indexOf('"', colon + 1);
        if (quote < 0) return "";
        int end = text.indexOf('"', quote + 1);
        if (end < 0) return "";
        String raw = text.substring(quote + 1, end);
        return raw.replace("\\\"", "\"").replace("\\\\", "\\").trim();
    }

    private static JSONObject parseJsonObject(String text) throws IOException {
        String value = text == null ? "" : text.trim();
        try {
            return new JSONObject(value);
        } catch (Exception ignored) {
            int start = value.indexOf('{');
            int end = value.lastIndexOf('}');
            if (start >= 0 && end > start) {
                try {
                    return new JSONObject(value.substring(start, end + 1));
                } catch (Exception ignoredAgain) {
                }
            }
            throw new IOException("cloud login service returned invalid JSON");
        }
    }

    private static String membersToken(JSONObject response) throws IOException {
        JSONObject data = response.optJSONObject("data");
        JSONObject members = data == null ? null : data.optJSONObject("members");
        String token = members == null ? "" : members.optString("token", "").trim();
        if (token.isEmpty()) throw new IOException("登录服务未返回二维码 Token");
        return token;
    }

    private static String cookieHeader(List<String> setCookies, String requiredText) {
        LinkedHashMap<String, String> values = new LinkedHashMap<>();
        for (String header : setCookies) {
            if (!requiredText.isEmpty() && !header.contains(requiredText)) continue;
            String pair = header.split(";", 2)[0].trim();
            int equals = pair.indexOf('=');
            if (equals <= 0) continue;
            values.put(pair.substring(0, equals), pair);
        }
        return String.join(";", values.values());
    }

    private static Class<?> findClass(ClassLoader loader, String... names) throws ClassNotFoundException {
        ClassNotFoundException failure = null;
        for (String name : names) {
            try {
                return Class.forName(name, true, loader);
            } catch (ClassNotFoundException error) {
                failure = error;
            }
        }
        throw failure == null ? new ClassNotFoundException("cloud provider class not found") : failure;
    }

    private static Throwable rootCause(Throwable error) {
        Throwable current = error;
        while (true) {
            if (current instanceof InvocationTargetException invocation && invocation.getTargetException() != null) {
                current = invocation.getTargetException();
                continue;
            }
            if (current.getCause() == null || current.getCause() == current) return current;
            current = current.getCause();
        }
    }

    private static String failureMessage(String provider, Throwable error) {
        String message = error.getMessage() == null ? "" : error.getMessage().trim();
        if (!message.isEmpty()
                && (message.contains("凭据") || message.contains("登录信息") || message.contains("扫码"))) {
            return message;
        }
        return providerLabel(provider) + "扫码确认后的登录交换失败";
    }

    private static String safeError(Throwable error) {
        String message = error.getMessage() == null ? "" : error.getMessage();
        message = message.replaceAll("https?://\\S+", "<url>");
        return message.length() <= 180 ? message : message.substring(0, 180);
    }

    private String newId() {
        byte[] bytes = new byte[16];
        random.nextBytes(bytes);
        StringBuilder id = new StringBuilder(32);
        for (byte value : bytes) id.append(String.format(Locale.ROOT, "%02x", value & 0xff));
        return id.toString();
    }

    private static String normalizeProvider(String provider) {
        String value = provider == null ? "" : provider.trim().toLowerCase(Locale.ROOT);
        if (!List.of("quark", "uc", "uctv", "baidu").contains(value)) {
            throw new IllegalArgumentException("unsupported cloud provider `" + provider + "`");
        }
        return value;
    }

    private static String providerLabel(String provider) {
        return switch (provider) {
            case "quark" -> "夸克";
            case "uc" -> "UC";
            case "uctv" -> "UC TV";
            case "baidu" -> "百度";
            default -> provider;
        };
    }

    private static final class Session {
        final String id;
        final String provider;
        final String token;
        final String qrText;
        final String qrImage;
        final ClassLoader jarLoader;
        final Class<?> baiduType;
        final String deviceId;
        final long expiresAt = System.currentTimeMillis() + SESSION_TTL.toMillis();
        int failures;

        Session(
                String id,
                String provider,
                String token,
                String qrText,
                String qrImage,
                ClassLoader jarLoader,
                Class<?> baiduType) {
            this(id, provider, token, qrText, qrImage, jarLoader, baiduType, "");
        }

        Session(
                String id,
                String provider,
                String token,
                String qrText,
                String qrImage,
                ClassLoader jarLoader,
                Class<?> baiduType,
                String deviceId) {
            this.id = id;
            this.provider = provider;
            this.token = token;
            this.qrText = qrText;
            this.qrImage = qrImage;
            this.jarLoader = jarLoader;
            this.baiduType = baiduType;
            this.deviceId = deviceId;
        }
    }
}
