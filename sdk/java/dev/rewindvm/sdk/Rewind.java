// SPDX-License-Identifier: MIT
package dev.rewindvm.sdk;

import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Supplier;

/**
 * The Rewind SDK for Java: assertions and test marks a program writes to
 * /dev/rewind, which {@code rewind check} judges on every run it tries.
 * Outside Rewind there is no /dev/rewind and every call does nothing, so
 * the calls can stay in production code. sdk/README.md says what each one
 * means.
 *
 * <pre>
 * Rewind.always(sold &lt;= 10, "never-oversell", () -&gt; "sold=" + sold);
 * Rewind.sometimes(sold == 10, "sold-out");
 * </pre>
 *
 * <p>An id has no spaces. Each id writes a pass or a hit once, and a
 * failure or a miss once, per process, since every write costs the VM an
 * exit. For JUnit 5, {@link RewindExtension} marks each test's start and
 * result.
 */
public final class Rewind {
    private static final String DEVICE = "/dev/rewind";
    private static final String PREFIX = "rewind-sdk 1";
    /** The device keeps a write of up to 4096 bytes as one mark. */
    private static final int LINE_MAX = 4000;

    /** How a test ended. */
    public enum Result {
        PASS("pass"),
        FAIL("fail"),
        SKIP("skip");

        private final String word;

        Result(String word) {
            this.word = word;
        }
    }

    /** The device, opened once; null outside Rewind. */
    private static final FileOutputStream device = open();

    /** The lines written once already. */
    private static final Set<String> sent = ConcurrentHashMap.newKeySet();

    private Rewind() {}

    private static FileOutputStream open() {
        try {
            return new FileOutputStream(DEVICE);
        } catch (IOException | SecurityException e) {
            return null;
        }
    }

    /** Writes {@code words} as one line, in one write. */
    private static void say(String words) {
        if (device == null) {
            return;
        }
        byte[] line = (PREFIX + " " + words.replace('\n', ' ')).getBytes(StandardCharsets.UTF_8);
        int length = Math.min(line.length, LINE_MAX);
        byte[] out = new byte[length + 1];
        System.arraycopy(line, 0, out, 0, length);
        out[length] = '\n';
        synchronized (Rewind.class) {
            try {
                device.write(out);
            } catch (IOException e) {
                // Nothing to tell: the marks are for Rewind alone.
            }
        }
    }

    /** Writes the line {@code words} gives the first time {@code key} is seen only. */
    private static void sayOnce(String key, Supplier<String> words) {
        if (device != null && sent.add(key)) {
            say(words.get());
        }
    }

    private static String with(String words, String message) {
        return message == null || message.isEmpty() ? words : words + " " + message;
    }

    /** {@code cond} holds every time the program gets here. Returns {@code cond}. */
    public static boolean always(boolean cond, String id) {
        return always(cond, id, () -> "");
    }

    /** As {@link #always(boolean, String)}, saying {@code message} when it fails. */
    public static boolean always(boolean cond, String id, Supplier<String> message) {
        if (cond) {
            sayOnce("always pass " + id, () -> "always pass " + id);
        } else {
            sayOnce("always fail " + id, () -> with("always fail " + id, message.get()));
        }
        return cond;
    }

    /** Some run should make {@code cond} true here. Returns {@code cond}. */
    public static boolean sometimes(boolean cond, String id) {
        String words = (cond ? "sometimes hit " : "sometimes miss ") + id;
        sayOnce(words, () -> words);
        return cond;
    }

    /** No run should get here. */
    public static void unreachable(String id, String message) {
        sayOnce("unreachable " + id, () -> with("unreachable " + id, message));
    }

    /** A test named {@code name}, which may have spaces, starts. */
    public static void testBegin(String name) {
        say("test-begin " + name);
    }

    /** The test named {@code name} ended as {@code result} says. */
    public static void testEnd(String name, Result result) {
        say("test-end " + result.word + " " + name);
    }
}
