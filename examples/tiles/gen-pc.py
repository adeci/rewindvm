"""Write tiles.pc, libtiles' pkg-config file, to standard output.

The packages libtiles links against are collected into a set, so each is
named once. Python iterates a set of strings in an order drawn from its
hash seed, so the Requires line can come out in another order each run.
"""

import sys

VERSION = "0.2.0"

# Each part of libtiles and the packages that part links against.
PARTS = {
    "decode": ["zlib", "libpng"],
    "render": ["pixman-1", "zlib"],
}

# Every package some part links against, once.
requires = set()
for deps in PARTS.values():
    requires.update(deps)

prefix = sys.argv[1]
print(f"prefix={prefix}")
print("libdir=${prefix}/lib")
print()
print("Name: tiles")
print("Description: Tile a large image into small ones")
print(f"Version: {VERSION}")
print(f"Requires.private: {', '.join(requires)}")
print("Libs: -L${libdir} -ltiles")
