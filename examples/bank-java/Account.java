import dev.rewindvm.sdk.Rewind;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * One account two tellers deposit into. A deposit reads the balance,
 * prints a line to the ledger, then stores the new balance, all without a
 * lock, so a teller that runs between another's read and store has its
 * deposit written over.
 */
final class Account {
    private long balance;

    /** How many tellers are inside deposit. */
    private final AtomicInteger depositing = new AtomicInteger();

    /** The race: the balance read here is stale by the time it is stored. */
    void deposit(int teller, long amount) {
        // Another teller is mid-deposit: the case a lock around the read and
        // store has to handle, and the only way to lose a deposit without one.
        int others = depositing.getAndIncrement();
        Rewind.sometimes(others > 0, "deposits-concurrent");

        long seen = balance;
        System.out.println("teller " + teller + ": " + seen + " + " + amount);
        balance = seen + amount;
        depositing.decrementAndGet();
    }

    long balance() {
        return balance;
    }
}
