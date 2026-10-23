"""Write the index of the plugins in a directory to standard output, one
plugin a line.

os.listdir returns a directory's entries in the order its filesystem keeps
them, and the script writes them in that order.
"""

import os
import sys

SUFFIX = ".ini"

directory = sys.argv[1]
print("# The plugins this package ships, one a line.")
for name in os.listdir(directory):
    # Only the plugins' own files are plugins.
    if not name.endswith(SUFFIX):
        continue

    print(name.removesuffix(SUFFIX))
