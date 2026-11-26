"""The idle form of `fool_thread.py`: a server thread waits on a socket while
the main thread waits for a key in cbreak mode. Nothing works and nothing asks."""
import os
import socket
import sys
import threading
import tty

port = int(sys.argv[1])


def serve():
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port))
    s.listen(4)
    while True:
        s.accept()[0].close()


threading.Thread(target=serve, daemon=True).start()
print(f"listening on {port}; press h for help", flush=True)
tty.setcbreak(0)
os.read(0, 1)
