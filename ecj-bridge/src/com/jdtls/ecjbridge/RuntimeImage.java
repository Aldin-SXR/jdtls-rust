package com.jdtls.ecjbridge;

import java.io.IOException;
import java.net.URI;
import java.nio.file.FileSystem;
import java.nio.file.FileSystems;
import java.nio.file.Path;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/** JDK image facts for the library selected by the Rust project model. */
final class RuntimeImage {
    private static final Map<Path, FileSystem> IMAGES = new ConcurrentHashMap<>();

    static boolean isImage(String library) {
        return Path.of(library).getFileName().toString().equals("jrt-fs.jar");
    }

    static FileSystem open(String library) throws IOException {
        Path home = Path.of(library).toAbsolutePath().getParent().getParent().toRealPath();
        FileSystem existing = IMAGES.get(home);
        if (existing != null) return existing;
        synchronized (IMAGES) {
            existing = IMAGES.get(home);
            if (existing == null) {
                existing = FileSystems.newFileSystem(URI.create("jrt:/"), Map.of("java.home", home.toString()));
                IMAGES.put(home, existing);
            }
            return existing;
        }
    }
}
