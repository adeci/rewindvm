// Two JUnit tests the extension marks, and the Java SDK's calls, for
// checks.sdk-languages.
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.rewindvm.sdk.Rewind;
import dev.rewindvm.sdk.RewindExtension;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;

@ExtendWith(RewindExtension.class)
class DemoTest {
    @Test
    void passes() {
        assertTrue(Rewind.always(true, "java-holds"));
        Rewind.sometimes(false, "java-sometimes");
    }

    @Test
    void fails() {
        for (int i = 0; i < 2; i++) {
            Rewind.always(false, "java-fails", () -> "java said so");
        }
        Rewind.unreachable("java-unreachable", "reached");
        assertTrue(false);
    }
}
