/* The C SDK's calls, for checks.sdk-languages. */
#include "rewind.h"

int main(void)
{
	rewind_test_begin("c demo");
	REWIND_ALWAYS(1, "c-holds");
	for (int i = 0; i < 2; i++)
		REWIND_ALWAYS(0, "c-fails", "%s said so", "c");
	REWIND_SOMETIMES(0, "c-sometimes");
	REWIND_UNREACHABLE("c-unreachable", "reached");
	rewind_test_end("c demo", REWIND_FAIL);
	return 0;
}
