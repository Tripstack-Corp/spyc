#!/usr/bin/env python3
"""Replay exact-version Codex question views through real spyc panes and MCP.

The children are controlled PTY fixtures, not native Codex or trusted hooks.
Every run uses a new named Ghostty session, private home and private state.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import socket
import subprocess
import sys
import time
import tempfile
import uuid

ROOT = Path(__file__).resolve().parent.parent

def report(metadata, status):
    """Report only to a pane belonging to this run's isolated spyc."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(10)
        connection.connect(metadata["socket"])
        stream = connection.makefile("rb")
        def rpc(method, params, ident):
            body = json.dumps({"jsonrpc":"2.0","id":ident,"method":method,"params":params}).encode()
            connection.sendall(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
            while True:
                headers = {}
                while True:
                    line = stream.readline()
                    if not line:
                        raise AssertionError("isolated spyc closed its MCP connection")
                    if line in (b"\r\n", b"\n"):
                        break
                    key, value = line.decode().split(":",1)
                    headers[key.lower()] = value.strip()
                answer = json.loads(stream.read(int(headers["content-length"])))
                if answer.get("id") == ident:
                    return answer
        rpc("initialize", {"protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"async-question replay","version":"1"},
            "_meta":{"spyc/paneId":metadata["pane_id"]}}, 1)
        response = rpc("tools/call", {"name":"report_status","arguments":{
            "status":status,"pane_id":metadata["pane_id"],"ttl_ms":120000}}, 2)
        assert not response.get("error") and not response.get("result",{}).get("isError"), response

def run(smoke, binary):
    output = smoke.output
    # macOS Unix socket paths must fit even when evidence is saved in a deep path.
    smoke.runtime = tempfile.TemporaryDirectory(prefix="spyc-aq-",dir="/tmp")
    runtime = Path(smoke.runtime.name)
    home = runtime/"home"; home.mkdir()
    project = runtime/"project"; project.mkdir()
    fakebin = runtime/"bin"; fakebin.mkdir()
    (home/".spycrc.toml").write_text('[pane]\nagent_hud="off"\ncodex_mcp=false\n[mouse]\ncapture=false\n')
    fixtures = ROOT/"tests/fixtures/codex-questions"
    agent = runtime/"fixture.py"
    agent.write_text(
        "import sys,os,json,termios\nfrom pathlib import Path\n"
        "output=Path(sys.argv[1]);fixtures=Path(sys.argv[2]);number=next(arg for arg in sys.argv[3:] if arg in ['1','2'])\n"
        "attrs=termios.tcgetattr(0);attrs[3]&=~termios.ECHO;termios.tcsetattr(0,termios.TCSANOW,attrs)\n"
        "(output/('pane-'+number+'.json')).write_text(json.dumps({'socket':os.environ['SPYC_MCP_SOCK'],'pane_id':os.environ['SPYC_PANE_ID']}))\n"
        "print('READY_'+number,flush=True)\n"
        "for line in sys.stdin:\n"
        " command=line.strip()\n"
        " if command in ['OPEN','COLLAPSE']:\n"
        "  text=(fixtures/('expanded-options.txt' if command=='OPEN' else 'collapsed.txt')).read_text()\n"
        " elif command=='ANSWER':text='ANSWERED_'+number+'\\n\\n› Ask Codex to do anything\\n\\n100% context left | Vim: Insert\\n'\n"
        " else:continue\n"
        " print('\\x1b[2J\\x1b[H'+text.replace('\\n','\\r\\n'),end='',flush=True)\n")
    launcher = fakebin/"codex"
    launcher.write_text("#!/bin/sh\nexec "+shlex.join([sys.executable,str(agent),str(output),str(fixtures)])+' "$@"\n')
    launcher.chmod(0o700)
    sessions = json.loads(subprocess.check_output(["tui-test","--json","sessions"],text=True))["sessions"]
    assert smoke.args.session not in sessions
    smoke.cli("run","--backend","ghostty","--cols","200","--rows","60","--cwd",str(project),
        "--env","HOME="+str(home),"--env","CODEX_HOME="+str(home/".codex"),
        "--env","XDG_STATE_HOME="+str(runtime/"state"),
        "--env","SHELL=/bin/sh","--env","PATH="+str(binary.parent)+os.pathsep+os.environ["PATH"],
        "--env","SPYC_MCP_SOCK=","--env","SPYC_PANE_ID=",str(binary),"--no-lua")
    smoke.started = True
    smoke.cli("record","start",str(output/"session.cast"),"--format","cast")
    smoke.wait("🌶️",15000)
    def open_pane(number):
        smoke.key("Ctrl+a","c");smoke.wait("pane command:",5000);smoke.key("Ctrl+u")
        smoke.cli("type",shlex.join([str(launcher),str(number)]));smoke.key("Enter")
        smoke.wait("pane cwd:",5000);smoke.key("Enter");smoke.wait("READY_"+str(number),10000)
        if "spyc — agent status" in smoke.capture("startup-"+str(number)):
            smoke.key("n")
        return json.loads((output/("pane-"+str(number)+".json")).read_text())
    def send(command, expected):
        smoke.cli("type",command);smoke.key("Enter");smoke.wait(expected,5000)
    def attention(name, pane=1):
        dump = smoke.wait_dump(name,"blocked",pane=pane)
        view = smoke.pane(dump,pane)
        assert "source: SCRAPE-FALLBACK status=blocked (awaiting async question answer)" in view, dump
        return dump
    first = open_pane(1);report(first,"working")
    send("OPEN","⌃] skip");attention("expanded")
    report(first,"blocked")
    semantic = smoke.wait_dump("semantic-block","blocked",pane=1)
    assert "source: SELF-REPORT status=blocked" in smoke.pane(semantic,1)
    report(first,"working");attention("working-without-repaint")
    send("COLLAPSE","to answer");attention("collapsed")
    second = open_pane(2);report(second,"working")
    send("OPEN","⌃] skip");both = attention("both-open",2)
    assert "dot=blocked" in smoke.pane(both,1)
    assert first["pane_id"] != second["pane_id"]
    send("ANSWER","ANSWERED_2")
    returned = smoke.wait_dump("second-answered","working",pane=2)
    assert "source: SELF-REPORT status=working" in smoke.pane(returned,2)
    assert "dot=blocked" in smoke.pane(returned,1)
    report(second,"done")
    smoke.key("Ctrl+a","1");attention("first-still-open")
    # The native view owns retirement; a newer semantic done stays underneath.
    report(first,"done");attention("done-under-question")
    smoke.key("Ctrl+a","k");smoke.cli("type",":activity transparent");smoke.key("Enter")
    smoke.wait(" dps ",5000);time.sleep(7)
    idle = smoke.capture("idle")
    assert re.search(r"\b0 dps\b",idle),idle
    smoke.cli("type",":activity");smoke.key("Enter");smoke.key("Ctrl+a","j")
    send("ANSWER","ANSWERED_1")
    done = smoke.wait_dump("first-answered","done",pane=1)
    assert "source: SELF-REPORT status=done" in smoke.pane(done,1)
    return {"idle_draws_per_second":0,"panes":2}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--session",default="spyc-async-"+uuid.uuid4().hex[:10])
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("smoke",ROOT/"scripts/codex-tui-smoke.py")
    module = importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    smoke = module.Smoke(args)
    binary = args.binary.resolve()
    result = {"passed":False,"coverage":"real spyc/Ghostty, controlled PTY views and pane-bound MCP; not native CLI/hooks",
        "binary":str(binary),"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),
        "version":subprocess.check_output([str(binary),"--version"],text=True).strip()}
    try:
        result.update(run(smoke,binary));result["passed"]=True
    except Exception as error:
        result["error"]=str(error)
        if smoke.started:
            smoke.capture("failure")
        print(str(error),file=sys.stderr,flush=True)
    finally:
        if smoke.started:
            smoke.cli("record","stop",check=False);smoke.cli("close",check=False)
        if getattr(smoke,"runtime",None):
            smoke.runtime.cleanup()
        (smoke.output/"result.json").write_text(json.dumps(result,indent=2)+"\n")
    print(f"{'PASS' if result['passed'] else 'FAIL'}: {smoke.output}",flush=True)
    return 0 if result["passed"] else 1

if __name__ == "__main__":
    sys.exit(main())
