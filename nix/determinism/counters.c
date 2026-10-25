/* Counts a loop's CPU cycles and instructions with perf_event_open, as a
 * performance test would, and prints each count or why it has none. A
 * guest that reaches the host's counters prints counts that change from
 * run to run. */
#include <errno.h>
#include <linux/perf_event.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>

static void count(const char *name, uint64_t config)
{
  struct perf_event_attr attr;
  memset(&attr, 0, sizeof(attr));
  attr.size = sizeof(attr);
  attr.type = PERF_TYPE_HARDWARE;
  attr.config = config;
  attr.exclude_kernel = 1;
  int fd = syscall(SYS_perf_event_open, &attr, 0, -1, -1, 0);
  if (fd < 0) {
    printf("%s: none, %s\n", name, strerror(errno));
    return;
  }

  /* The loop being measured. */
  volatile uint64_t x = 0;
  for (int i = 0; i < 100000; i++) {
    x += i;
  }

  uint64_t n = 0;
  if (read(fd, &n, sizeof(n)) != sizeof(n)) {
    printf("%s: unreadable\n", name);
  } else {
    printf("%s: %llu\n", name, (unsigned long long)n);
  }
  close(fd);
}

int main(void)
{
  count("cycles", PERF_COUNT_HW_CPU_CYCLES);
  count("instructions", PERF_COUNT_HW_INSTRUCTIONS);
  return 0;
}
