/* Draws a hardware random number with rdrand and keeps it in a global,
 * sleeps 200 times, and prints a fixed word: nothing the program does
 * after the draw depends on the number, so only the machine's memory
 * differs between two runs. */
#include <immintrin.h>
#include <stdio.h>
#include <unistd.h>

unsigned long long secret;

int main(void)
{
  unsigned long long v = 0;
  _rdrand64_step(&v);
  secret = v;
  for (int i = 0; i < 200; i++) {
    usleep(1000);
  }
  printf("done\n");
  return 0;
}
