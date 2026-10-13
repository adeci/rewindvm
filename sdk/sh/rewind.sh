# SPDX-License-Identifier: MIT
#
# The Rewind SDK for shell scripts: assertions and test marks a script
# writes to /dev/rewind, which `rewind check` judges on every run it
# tries. Outside Rewind there is no /dev/rewind and every call does
# nothing but run its command. sdk/README.md says what each one means.
#
#   . ./rewind.sh
#   rows=$(sqlite3 "$db" 'select count(*) from users')
#   rewind_always users-kept "rows=$rows" test "$rows" -eq 3
#   rewind_sometimes cache-was-warm test -s "$cache"
#   rewind_test_begin "migration keeps users"
#   rewind_test_end pass "migration keeps users"
#
# An id has no spaces. rewind_always and rewind_sometimes run the command
# after the id (and the message) and return its status. Each id writes a
# pass or a hit once, and a failure or a miss once, per shell, since every
# write costs the VM an exit.

REWIND_DEVICE=/dev/rewind
# The lines written once already, by key. Every variable this file sets
# starts with REWIND_ or rewind_, so a script's own are left alone.
REWIND_SENT=" "

# Writes one line to the device, when there is one.
rewind_say() {
  [ -w "$REWIND_DEVICE" ] || return 0
  printf 'rewind-sdk 1 %s\n' "$*" > "$REWIND_DEVICE"
}

# rewind_say_once KEY WORDS...: writes WORDS the first time KEY is seen.
rewind_say_once() {
  case "$REWIND_SENT" in
    *" $1 "*) return 0 ;;
  esac
  REWIND_SENT="$REWIND_SENT$1 "
  shift
  rewind_say "$@"
}

# rewind_always ID MESSAGE COMMAND...: COMMAND succeeds every time.
rewind_always() {
  rewind_id=$1 rewind_message=$2
  shift 2
  if "$@"; then
    rewind_say_once "always-pass:$rewind_id" always pass "$rewind_id"
    return 0
  else
    rewind_status=$?
    rewind_say_once "always-fail:$rewind_id" always fail "$rewind_id" "$rewind_message"
    return $rewind_status
  fi
}

# rewind_sometimes ID COMMAND...: some run makes COMMAND succeed.
rewind_sometimes() {
  rewind_id=$1
  shift
  if "$@"; then
    rewind_say_once "sometimes-hit:$rewind_id" sometimes hit "$rewind_id"
    return 0
  else
    rewind_status=$?
    rewind_say_once "sometimes-miss:$rewind_id" sometimes miss "$rewind_id"
    return $rewind_status
  fi
}

# rewind_unreachable ID [MESSAGE]: no run gets here.
rewind_unreachable() {
  rewind_say_once "unreachable:$1" unreachable "$@"
}

# rewind_test_begin NAME: a test starts.
rewind_test_begin() {
  rewind_say test-begin "$*"
}

# rewind_test_end pass|fail|skip NAME: the test ended.
rewind_test_end() {
  rewind_say test-end "$@"
}
