/* SPDX-License-Identifier: MIT */
/*
 * The Rewind SDK for C and C++: assertions and test marks a program writes
 * to /dev/rewind, which `rewind check` judges on every run it tries.
 * Outside Rewind there is no /dev/rewind and every call does nothing, so
 * the calls can stay in production code. sdk/README.md says what each
 * one means.
 *
 *	#include "rewind.h"
 *
 *	REWIND_ALWAYS(balance == expected, "balance-conserved",
 *		      "balance=%ld expected=%ld", balance, expected);
 *	REWIND_SOMETIMES(others > 0, "deposits-concurrent");
 *	REWIND_UNREACHABLE("pay-after-close", "account %d", id);
 *
 * An id is a string literal without spaces. The message, a printf format
 * and its arguments, is optional and formatted only when written. Each
 * call site writes a pass or a hit once, and a failure or a miss once,
 * per process, since every write costs the VM an exit.
 */
#ifndef REWIND_SDK_H
#define REWIND_SDK_H

#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define REWIND_DEVICE "/dev/rewind"
/* The device keeps a write of up to 4096 bytes as one mark. */
#define REWIND_LINE_MAX 4000
/* What every line starts with: the SDK's word and the protocol version. */
#define REWIND_PREFIX "rewind-sdk 1 "
/* An fd not opened yet, as opposed to -1, not there. */
#define REWIND_UNOPENED (-2)

/* /dev/rewind, opened once per program; -1 outside Rewind. */
static inline int rewind_fd(void)
{
	static int fd = REWIND_UNOPENED;
	int seen = __atomic_load_n(&fd, __ATOMIC_ACQUIRE);

	if (seen != REWIND_UNOPENED)
		return seen;
	int opened = open(REWIND_DEVICE, O_WRONLY | O_CLOEXEC);
	int expected = REWIND_UNOPENED;

	if (!__atomic_compare_exchange_n(&fd, &expected, opened, 0, __ATOMIC_ACQ_REL,
					 __ATOMIC_ACQUIRE)) {
		/* Another thread opened it first. */
		if (opened >= 0)
			close(opened);
		return expected;
	}
	return opened;
}

/*
 * Writes one line in a single write: the prefix, `words`, and the message
 * `fmt` formats, if it is not empty, with its newlines made spaces.
 */
static inline void rewind_say(const char *words, const char *fmt, ...)
{
	int fd = rewind_fd();

	if (fd < 0)
		return;
	char line[REWIND_LINE_MAX];
	int n = snprintf(line, sizeof line - 1, REWIND_PREFIX "%s", words);

	if (n < 0)
		return;
	if (n > (int)sizeof line - 2)
		n = sizeof line - 2;

	/* The message, after a space. */
	if (fmt[0] != '\0' && n < (int)sizeof line - 3) {
		va_list ap;
		int start = n + 1;

		line[n] = ' ';
		va_start(ap, fmt);
		int m = vsnprintf(line + start, sizeof line - 1 - start, fmt, ap);
		va_end(ap);
		if (m > 0) {
			n = start + m;
			if (n > (int)sizeof line - 2)
				n = sizeof line - 2;
			for (char *c = line + start; c < line + n; c++) {
				if (*c == '\n')
					*c = ' ';
			}
		}
	}
	line[n++] = '\n';
	(void)!write(fd, line, n);
}

/* Whether this is the first time `bit` is set in `sent`. */
static inline int rewind_first(int *sent, int bit)
{
	return !(__atomic_fetch_or(sent, bit, __ATOMIC_RELAXED) & bit);
}

#define REWIND_SENT_PASS 1
#define REWIND_SENT_FAIL 2

/* `cond` holds every time the program gets here. */
#define REWIND_ALWAYS(cond, id, ...)                                                   \
	do {                                                                           \
		static int rewind_sent_;                                               \
		if (cond) {                                                            \
			if (rewind_first(&rewind_sent_, REWIND_SENT_PASS))             \
				rewind_say("always pass " id, "");                     \
		} else if (rewind_first(&rewind_sent_, REWIND_SENT_FAIL)) {            \
			rewind_say("always fail " id, "" __VA_ARGS__);                 \
		}                                                                      \
	} while (0)

/* Some run should make `cond` true here. */
#define REWIND_SOMETIMES(cond, id)                                                     \
	do {                                                                           \
		static int rewind_sent_;                                               \
		if (cond) {                                                            \
			if (rewind_first(&rewind_sent_, REWIND_SENT_PASS))             \
				rewind_say("sometimes hit " id, "");                   \
		} else if (rewind_first(&rewind_sent_, REWIND_SENT_FAIL)) {            \
			rewind_say("sometimes miss " id, "");                          \
		}                                                                      \
	} while (0)

/* No run should get here. */
#define REWIND_UNREACHABLE(id, ...)                                                    \
	do {                                                                           \
		static int rewind_sent_;                                               \
		if (rewind_first(&rewind_sent_, REWIND_SENT_FAIL))                     \
			rewind_say("unreachable " id, "" __VA_ARGS__);                 \
	} while (0)

/* How a test ended. */
enum rewind_result { REWIND_PASS, REWIND_FAIL, REWIND_SKIP };

/* A test named `name`, which may have spaces, starts. */
static inline void rewind_test_begin(const char *name)
{
	rewind_say("test-begin", "%s", name);
}

/* The test named `name` ended as `result` says. */
static inline void rewind_test_end(const char *name, enum rewind_result result)
{
	const char *words = "test-end fail";

	switch (result) {
	case REWIND_PASS:
		words = "test-end pass";
		break;
	case REWIND_SKIP:
		words = "test-end skip";
		break;
	case REWIND_FAIL:
		break;
	}
	rewind_say(words, "%s", name);
}

#endif
