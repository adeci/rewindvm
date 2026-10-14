import dev.rewindvm.sdk.Rewind;

/**
 * One account two tellers deposit into. A deposit reads the balance,
 * prints a line to the ledger, then stores the new balance, all without a
 * lock, so a teller that runs between another's read and store has its
 * deposit written over.
 */
final class Account {
    private long balance;

    /** The race: the balance read here is stale by the time it is stored. */
    void deposit(int teller, long amount) {
        long seen = balance;
        System.out.println("teller " + teller + ": " + seen + " + " + amount);

        // Another teller stored between this one's read and store.
        Rewind.sometimes(balance != seen, "deposits-overlapped");
        balance = seen + amount;
    }

    long balance() {
        return balance;
    }
}
