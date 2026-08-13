package com.fongmi.webhtv.java;

import com.googlecode.d2j.dex.Dex2jar;
import com.googlecode.d2j.reader.BaseDexFileReader;
import com.googlecode.d2j.reader.MultiDexFileReader;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;

final class DexConverter {
    private final Path cacheDir;

    DexConverter(Path cacheDir) throws IOException {
        this.cacheDir = cacheDir;
        Files.createDirectories(cacheDir);
    }

    Path convert(Path sourceJar) throws IOException {
        Path absolute = sourceJar.toAbsolutePath().normalize();
        if (!Files.isRegularFile(absolute)) {
            throw new IOException("Spider jar does not exist: " + absolute);
        }
        String digest = sha256(absolute);
        Path output = cacheDir.resolve(digest + "-dex2jar.jar");
        if (Files.isRegularFile(output) && Files.size(output) > 0) {
            return output;
        }
        Path temp = cacheDir.resolve(digest + "-dex2jar.tmp.jar");
        Files.deleteIfExists(temp);
        BaseDexFileReader reader = MultiDexFileReader.open(Files.readAllBytes(absolute));
        Dex2jar.from(reader)
                .reUseReg(false)
                .topoLogicalSort()
                .skipDebug(false)
                .optimizeSynchronized(false)
                .printIR(false)
                .noCode(false)
                .skipExceptions(false)
                .dontSanitizeNames(false)
                .computeFrames(false)
                .to(temp);
        Files.move(temp, output, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
        return output;
    }

    private static String sha256(Path path) throws IOException {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            digest.update(Files.readAllBytes(path));
            return HexFormat.of().formatHex(digest.digest());
        } catch (Exception error) {
            throw new IOException("unable to hash jar: " + error.getMessage(), error);
        }
    }
}
