# The Python SDK's calls, for checks.sdk-languages.
import rewind_sdk as rewind

rewind.test_begin("python demo")
rewind.always(True, "python-holds")
for _ in range(2):
    rewind.always(False, "python-fails", "python said so")
rewind.sometimes(False, "python-sometimes")
rewind.unreachable("python-unreachable", "reached")
rewind.test_end("python demo", rewind.FAIL)
