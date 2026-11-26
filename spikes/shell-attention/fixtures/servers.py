"""Local servers for the corpus: `401 PORT` answers every request with HTTP 401
and a Basic challenge (git's credential prompt); `stall PORT` accepts and never
answers (curl waiting on a socket). Bound to 127.0.0.1 only."""
import socket
import sys

mode, port = sys.argv[1], int(sys.argv[2])
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", port))
s.listen(16)
held = []
while True:
    c, _ = s.accept()
    if mode == "stall":
        held.append(c)
        continue
    c.recv(65536)
    c.sendall(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"spike\"\r\n"
              b"Content-Length: 0\r\nConnection: close\r\n\r\n")
    c.close()
