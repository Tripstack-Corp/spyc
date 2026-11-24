import os, fcntl, sys
d = sys.argv[1]
fd = os.open(d, os.O_RDONLY)
try:
    r = fcntl.fcntl(fd, 51)  # F_FULLFSYNC
    print("F_FULLFSYNC on dir ok", r)
except OSError as e:
    print("F_FULLFSYNC on dir failed", e)
try:
    os.fsync(fd); print("fsync on dir ok")
except OSError as e:
    print("fsync on dir failed", e)
