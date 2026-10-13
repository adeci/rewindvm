# The shell SDK's calls, for checks.sdk-languages.
. "$1"
rewind_test_begin "sh demo"
rewind_always sh-holds "" true
for i in 1 2; do rewind_always sh-fails "sh said so" false || true; done
rewind_sometimes sh-sometimes false || true
rewind_unreachable sh-unreachable reached
rewind_test_end fail "sh demo"
