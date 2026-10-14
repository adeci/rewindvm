import static org.junit.jupiter.api.Assertions.assertEquals;

import dev.rewindvm.sdk.Rewind;
import dev.rewindvm.sdk.RewindExtension;
import org.junit.jupiter.api.MethodOrderer;
import org.junit.jupiter.api.Order;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestMethodOrder;
import org.junit.jupiter.api.extension.ExtendWith;

/**
 * Tests of Account, one teller and then two. RewindExtension, the Rewind
 * SDK's JUnit 5 extension, marks each test's start and result, so
 * `rewind check` fails a run in which a test that passed under schedule 0
 * fails. Outside Rewind it does nothing.
 */
@ExtendWith(RewindExtension.class)
@TestMethodOrder(MethodOrderer.OrderAnnotation.class)
class BankTest {
    static final int DEPOSITS = 20;
    static final long AMOUNT = 10;

    /** Deposits by `teller`, DEPOSITS of AMOUNT each. */
    static Thread teller(Account account, int teller) {
        return new Thread(() -> {
            for (int i = 0; i < DEPOSITS; i++) {
                account.deposit(teller, AMOUNT);
            }
        });
    }

    // One teller alone: nothing to race with, so every deposit counts.
    @Test
    @Order(1)
    void oneTellerDeposits() throws InterruptedException {
        Account account = new Account();
        Thread first = teller(account, 1);
        first.start();
        first.join();
        assertEquals(DEPOSITS * AMOUNT, account.balance());
    }

    // Two tellers at once: the balance adds up only if no deposit was
    // written over.
    @Test
    @Order(2)
    void twoTellersDeposit() throws InterruptedException {
        Account account = new Account();
        Thread first = teller(account, 1);
        Thread second = teller(account, 2);
        first.start();
        second.start();
        first.join();
        second.join();

        long expected = 2 * DEPOSITS * AMOUNT;
        long balance = account.balance();
        Rewind.always(balance == expected, "balance-adds-up",
                () -> "balance " + balance + ", expected " + expected);
        assertEquals(expected, balance);
    }

    // A new account starts empty.
    @Test
    @Order(3)
    void newAccountIsEmpty() {
        assertEquals(0, new Account().balance());
    }
}
