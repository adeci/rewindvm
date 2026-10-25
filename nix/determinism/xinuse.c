/* Prints XINUSE, which xgetbv(1) reads: which parts of the extended
 * register state are in use. The CPU model does not advertise
 * XGETBV_ECX1, but the instruction runs in the guest anyway. Before and
 * after touching the AVX registers, and after a sleep, which lets the
 * kernel switch tasks and restore the state. */
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>

static uint32_t xinuse(void)
{
  uint32_t lo, hi;
  __asm__ volatile("xgetbv" : "=a"(lo), "=d"(hi) : "c"(1));
  return lo;
}

int main(void)
{
  printf("xinuse at start: %#x\n", xinuse());
  __asm__ volatile("vpxor %%ymm1, %%ymm1, %%ymm1" ::: "xmm1");
  __asm__ volatile("vpcmpeqd %%ymm2, %%ymm2, %%ymm2" ::: "xmm2");
  printf("xinuse after AVX: %#x\n", xinuse());
  usleep(1000);
  printf("xinuse after a sleep: %#x\n", xinuse());
  return 0;
}
