/*
 * Two threads count hits on one counter. A hit reads the count and stores
 * the count plus one, with no lock, so a thread that runs between
 * another's read and store has its hit written over. The check counts
 * hits from both threads and fails when the total comes up short.
 *
 * Nothing between the read and the store makes a system call. Rewind VM
 * switches threads only where a program makes one, so no schedule reaches
 * this race until the schedule points tutorial marks the place with the
 * Rewind SDK's rewind_point. rewind.h is the SDK's header, copied from
 * sdk/c in Rewind VM's repository; it also tells `rewind check` that the
 * total must add up and marks the test's start and result. Outside Rewind
 * its calls do nothing.
 */
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>

#include "rewind.h"

#define THREADS 2
#define HITS 10

static atomic_long hits;

/* The race: the count read here is stale by the time it is stored. */
static void hit(void)
{
	long seen = atomic_load(&hits);
	atomic_store(&hits, seen + 1);
}

static void *count(void *arg)
{
	for (int i = 0; i < HITS; i++)
		hit();
	return NULL;
}

/* The test's name, as the Rewind SDK marks it. */
#define TEST "two threads count"

int main(void)
{
	pthread_t threads[THREADS];
	long expected = THREADS * HITS;

	rewind_test_begin(TEST);
	for (int i = 0; i < THREADS; i++)
		pthread_create(&threads[i], NULL, count, NULL);
	for (int i = 0; i < THREADS; i++)
		pthread_join(threads[i], NULL);

	long total = atomic_load(&hits);
	printf("%ld hits, expected %ld\n", total, expected);
	REWIND_ALWAYS(total == expected, "hits-add-up", "%ld hits, expected %ld", total, expected);
	rewind_test_end(TEST, total == expected ? REWIND_PASS : REWIND_FAIL);
	return total == expected ? 0 : 1;
}
