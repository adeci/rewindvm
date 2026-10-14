/*
 * Two tellers deposit into one account. A deposit reads the balance,
 * writes a line to the ledger, then stores the new balance, all without a
 * lock, so a teller that runs between another's read and store has its
 * deposit written over. The check has both tellers deposit and fails when
 * the balance comes up short.
 *
 * rewind.h is the Rewind SDK's header, copied from sdk/c in Rewind VM's
 * repository: it tells `rewind check` that the balance must add up, and
 * that some run should have both tellers mid-deposit at once, and marks
 * the test's start and result. Outside Rewind its calls do nothing.
 */
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <unistd.h>

#include "rewind.h"

#define DEPOSITS 20
#define AMOUNT 10

static long balance;

/* How many tellers are inside deposit. */
static atomic_int depositing;

/* The race: the balance read here is stale by the time it is stored. */
static void deposit(int teller, long amount)
{
	/* Another teller is mid-deposit: the case a lock around the read and
	 * store has to handle, and the only way to lose a deposit without one. */
	int others = atomic_fetch_add(&depositing, 1);
	REWIND_SOMETIMES(others > 0, "deposits-concurrent");

	long seen = balance;
	char line[64];
	int n = snprintf(line, sizeof line, "teller %d: %ld + %ld\n", teller, seen, amount);

	if (write(1, line, n) == n) {
		balance = seen + amount;
	}
	atomic_fetch_sub(&depositing, 1);
}

static void *teller(void *arg)
{
	int id = (int)(long)arg;

	for (int i = 0; i < DEPOSITS; i++)
		deposit(id, AMOUNT);
	return NULL;
}

/* The test's name, as the Rewind SDK marks it. */
#define TEST "two tellers deposit"

int main(void)
{
	pthread_t first, second;
	long expected = 2L * DEPOSITS * AMOUNT;

	rewind_test_begin(TEST);
	pthread_create(&first, NULL, teller, (void *)1L);
	pthread_create(&second, NULL, teller, (void *)2L);
	pthread_join(first, NULL);
	pthread_join(second, NULL);

	printf("balance %ld, expected %ld\n", balance, expected);
	REWIND_ALWAYS(balance == expected, "balance-adds-up", "balance %ld, expected %ld", balance,
		      expected);
	rewind_test_end(TEST, balance == expected ? REWIND_PASS : REWIND_FAIL);
	return balance == expected ? 0 : 1;
}
