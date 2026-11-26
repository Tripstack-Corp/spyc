"""Built to fool S1 and S2: holds the terminal in raw mode and polls it every
50 ms while it works, as a full-screen tool with a progress view does."""
import os
import select
import time
import tty

tty.setraw(0)
x = 0
while True:
    r, _, _ = select.select([0], [], [], 0.05)
    if r:
        os.read(0, 1)
    t = time.monotonic()
    while time.monotonic() - t < 0.04:
        x += 1
