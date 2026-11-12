import os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from term import Term
D = os.path.dirname(os.path.abspath(__file__))
SPYC = os.path.expanduser("~/.local/bin/spyc")
DTACH = "/opt/homebrew/bin/dtach"
def env_for(home):
    e = {k: v for k, v in os.environ.items()
         if not k.startswith(("SPYC", "CLAUDE", "XDG", "ZDOTDIR", "ANTHROPIC", "TMUX"))}
    e.update(HOME=home, TERM="xterm-256color", COLORTERM="truecolor", SHELL="/bin/zsh")
    return e
def show(t, title):
    print(f"--- {title}: {t.nonblank()} non-blank rows, modes set {t.set_modes()}, kitty depth {t.kitty_depth}, queries {t.queries}")

def wait_for(t, text, secs=30):
    """Pump `t` until `text` is on its screen."""
    end = time.monotonic() + secs
    while text not in t.text():
        assert time.monotonic() < end, f"never saw {text!r}:\n{t.text()}"
        t.pump(0.2)

def kill_and_reap(t, secs=15):
    """SIGKILL `t`'s program and reap it, reading its output meanwhile: a
    process exiting can block closing a terminal whose output nobody reads."""
    os.kill(t.pid, 9)
    end = time.monotonic() + secs
    while time.monotonic() < end:
        try:
            os.read(t.fd, 65536)
        except OSError:
            pass
        pid, _ = os.waitpid(t.pid, os.WNOHANG)
        if pid:
            return
        time.sleep(0.05)
    raise TimeoutError("the killed program never exited")

def token_file(home, socket, state=None):
    """Where spyc keeps the token of the jar at `socket` (R26)."""
    root = os.path.join(state, "spyc") if state else os.path.join(home, ".local", "state", "spyc")
    return os.path.join(root, "jar-tokens", os.path.splitext(os.path.basename(socket))[0])
