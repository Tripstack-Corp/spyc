"""Built to fool S2: one thread works while the main thread blocks reading the
terminal for a single key, as a dev server with a "press h for help" key does."""
import os
import sys
import termios
import threading
import tty


def work():
    x = 0
    while True:
        x += 1


threading.Thread(target=work, daemon=True).start()
print("serving; press h for help", flush=True)
tty.setcbreak(0)
os.read(0, 1)
