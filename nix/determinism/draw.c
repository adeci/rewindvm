/* Draws a hardware random number with rdrand, sleeps, and exits with a
 * status of 0 to 199 picked from the number: on a CPU that runs rdrand,
 * nearly every run ends differently, which `rewind check` sees as
 * schedules that end differently. */
#include <immintrin.h>
#include <unistd.h>

int main(void)
{
  unsigned long long v = 0;
  _rdrand64_step(&v);
  for (int i = 0; i < 20; i++) {
    usleep(1000);
  }
  return v % 200;
}
