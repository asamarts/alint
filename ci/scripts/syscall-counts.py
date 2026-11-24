#!/usr/bin/env python3
"""Count read- and stat-family system calls in an `strace -f` log.

Only the traced process and its threads count. A pid that calls `execve` after
the first one is a spawned child (`git`, `true`), whose calls vary by host, so
its lines are skipped. The resumed half of an interrupted call (`<... read
resumed>`) was already counted where it started.

A stat-family call on a file named `.git`, `.jj`, `.gitignore` or `.ignore` is
counted separately as `probe` (historically subject to ADR-0045's per-directory
allowance, now held to the same flat 32-call limit after #573 replaced
`ignore::WalkBuilder` with the owned parallel walker).

A log that cannot be a measurement fails (exit 1) rather than counting zero:
no syscall lines, no initial `execve` of the traced program, or a family other
than `probe` with no calls at all (every alint run reads, stats, opens, and
lists directories).

    syscall-counts.py --syscalls   print the strace `-e trace=` list
    syscall-counts.py LOG          print `family count` lines for LOG

Used by reference-perf-gate.sh (ADR-0023, V21-6; ADR-0045).
"""

import re
import sys

FAMILIES = {
    "read": {"read", "pread64", "readv", "preadv", "preadv2"},
    "stat": {
        "newfstatat", "fstat", "statx", "lstat", "stat",
        "access", "faccessat", "faccessat2", "readlink", "readlinkat",
        "statfs", "fstatfs", "getxattr", "lgetxattr", "fgetxattr",
    },
    "open": {"openat", "openat2", "open"},
    "dents": {"getdents64", "getdents"},
}
PROBED_NAMES = {".git", ".jj", ".gitignore", ".ignore"}
# `?` tells strace to ignore a name this architecture does not have.
TRACED = ["execve"] + sorted("?" + name for name in set().union(*FAMILIES.values()))


class NotAMeasurement(Exception):
    pass


def probed_name(arguments):
    """Whether a call's first path argument names a file in `PROBED_NAMES`."""
    path = re.search(r'"((?:[^"\\]|\\.)*)"', arguments)
    return path is not None and path.group(1).rsplit("/", 1)[-1] in PROBED_NAMES


def count(lines):
    rows = [line.rstrip("\n") for line in lines if re.match(r"^\d+ ", line)]
    if not rows:
        raise NotAMeasurement("the trace has no system calls")
    main, first = rows[0].split(" ", 1)
    if not first.lstrip().startswith("execve("):
        raise NotAMeasurement("the trace does not start with the program's execve")
    children = {
        row.split(" ", 1)[0]
        for row in rows
        if " execve(" in row and row.split(" ", 1)[0] != main
    }
    totals = {family: 0 for family in FAMILIES}
    totals["probe"] = 0
    for row in rows:
        pid, rest = row.split(" ", 1)
        rest = rest.lstrip()
        if pid in children or rest.startswith("<..."):
            continue
        call = re.match(r"(\w+)\(", rest)
        if not call:
            continue
        family = next((family for family, names in FAMILIES.items() if call.group(1) in names), None)
        if family == "stat" and probed_name(rest[call.end():]):
            family = "probe"
        if family:
            totals[family] += 1
    empty = [family for family, total in totals.items() if total == 0 and family != "probe"]
    if empty:
        raise NotAMeasurement(f"no {', '.join(empty)} calls: the run did not do its work")
    return totals


def main(argv):
    if argv == ["--syscalls"]:
        print(",".join(TRACED))
        return 0
    if len(argv) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[0], encoding="utf-8", errors="replace") as log:
        try:
            totals = count(log)
        except NotAMeasurement as error:
            print(f"syscall-counts: {argv[0]}: {error}", file=sys.stderr)
            return 1
    for family, total in totals.items():
        print(family, total)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
