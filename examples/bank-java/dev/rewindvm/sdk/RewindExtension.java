// SPDX-License-Identifier: MIT
package dev.rewindvm.sdk;

import java.util.Optional;
import org.junit.jupiter.api.extension.BeforeTestExecutionCallback;
import org.junit.jupiter.api.extension.ExtensionContext;
import org.junit.jupiter.api.extension.TestWatcher;

/**
 * A JUnit 5 extension that marks each test's start and result for Rewind,
 * named {@code Class.method}:
 *
 * <pre>
 * &#64;ExtendWith(RewindExtension.class)
 * class InventoryTest { ... }
 * </pre>
 */
public final class RewindExtension implements BeforeTestExecutionCallback, TestWatcher {
    private static String name(ExtensionContext context) {
        return context.getRequiredTestClass().getSimpleName() + "." + context.getRequiredTestMethod().getName();
    }

    @Override
    public void beforeTestExecution(ExtensionContext context) {
        Rewind.testBegin(name(context));
    }

    @Override
    public void testSuccessful(ExtensionContext context) {
        Rewind.testEnd(name(context), Rewind.Result.PASS);
    }

    @Override
    public void testFailed(ExtensionContext context, Throwable cause) {
        Rewind.testEnd(name(context), Rewind.Result.FAIL);
    }

    @Override
    public void testAborted(ExtensionContext context, Throwable cause) {
        Rewind.testEnd(name(context), Rewind.Result.SKIP);
    }

    @Override
    public void testDisabled(ExtensionContext context, Optional<String> reason) {
        Rewind.testEnd(name(context), Rewind.Result.SKIP);
    }
}
