/* Prints results x86 leaves to the CPU: the flags MUL leaves undefined,
 * what BSF and BSR leave in their destination for a zero source, and
 * the approximations RCPPS and RSQRTPS make. One host gives the same
 * answers every run; another CPU generation of the same vendor may not,
 * and a run replayed there then goes another way. */
#include <immintrin.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
  /* MUL defines CF and OF only; SF, ZF, AF and PF are left to the CPU. */
  uint64_t flags, lo, hi;
  __asm__ volatile("movq $0x123456789, %%rax\n\t"
                   "movq $0x987654321, %%rcx\n\t"
                   "mulq %%rcx\n\t"
                   "pushfq\n\t"
                   "popq %0"
                   : "=r"(flags), "=a"(lo), "=d"(hi)
                   :
                   : "rcx", "cc");
  printf("mul flags: %#llx\n", (unsigned long long)(flags & 0xd5));

  /* BSF and BSR of zero leave their destination undefined; the CPU
   * usually leaves it as it was. */
  uint64_t bsf = 0x5a5a, bsr = 0xa5a5, zero = 0;
  __asm__ volatile("bsfq %1, %0" : "+r"(bsf) : "r"(zero) : "cc");
  __asm__ volatile("bsrq %1, %0" : "+r"(bsr) : "r"(zero) : "cc");
  printf("bsf of zero: %#llx\nbsr of zero: %#llx\n", (unsigned long long)bsf,
         (unsigned long long)bsr);

  /* RCPPS and RSQRTPS promise 12 bits; the rest depends on the CPU. */
  __m128 v = _mm_set_ps(3.0f, 7.0f, 11.0f, 13.0f);
  float rcp[4], rsqrt[4];
  _mm_storeu_ps(rcp, _mm_rcp_ps(v));
  _mm_storeu_ps(rsqrt, _mm_rsqrt_ps(v));
  for (int i = 0; i < 4; i++) {
    uint32_t a, b;
    memcpy(&a, &rcp[i], 4);
    memcpy(&b, &rsqrt[i], 4);
    printf("rcpps %#x rsqrtps %#x\n", a, b);
  }
  return 0;
}
