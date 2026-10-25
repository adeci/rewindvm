/* Draws a hardware random number with rdrand, without asking CPUID
 * first, sleeps 200 times, then prints a bucket picked from the number.
 * Nothing in the run's inputs decides the number where the CPU runs the
 * instruction, as AMD's do; where the CPU faults it, as KVM on Intel
 * makes it when CPUID hides rdrand, the program dies of SIGILL. */
#include <immintrin.h>
#include <stdio.h>
#include <unistd.h>

int main(void)
{
  unsigned long long v = 0;
  _rdrand64_step(&v);
  for (int i = 0; i < 200; i++) {
    usleep(1000);
  }
  printf("bucket %llu\n", v % 1000);
  return 0;
}
