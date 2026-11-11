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
