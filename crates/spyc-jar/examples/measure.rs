//! Stage 2 measurements: `cargo run --release -p spyc-jar --example measure --
//! <jar binary>`. Prints keystroke-echo latency, throughput and replay time
//! through a jar against a PTY read directly, and the memory 30 jars take.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use spyc_jar::client::Client;
use spyc_jar::proto::{Msg, Role};

const ECHO: &str = "import os,tty\ntty.setraw(0)\nwhile True:\n b=os.read(0,64)\n if not b: break\n os.write(1,b)\n";

fn main() {
    let jar_bin: PathBuf = std::env::args().nth(1).expect("the spyc-jar binary").into();
    let dir = tempfile::Builder::new()
        .prefix("jm")
        .tempdir_in("/tmp")
        .unwrap();
    println!("latency, 1000 single-byte round trips through a raw echo program:");
    report("  direct PTY", &direct_latency());
    report("  through a jar", &jar_latency(&jar_bin, dir.path()));
    println!("throughput, 100 MB from the program to its reader:");
    println!("  direct PTY      {:7.0} MB/s", direct_throughput());
    println!(
        "  through a jar   {:7.0} MB/s",
        jar_throughput(&jar_bin, dir.path())
    );
    let (bytes, took) = replay(&jar_bin, dir.path());
    println!("replay: a new owner takes {bytes} logged bytes in {took:?}");
    let (rss_kb, n) = thirty_jars(&jar_bin, dir.path());
    println!(
        "memory: {n} idle jars, {} KB resident each on average, {} MB in all",
        rss_kb / n,
        rss_kb / 1024
    );
}

fn report(label: &str, samples: &[Duration]) {
    let mut s = samples.to_vec();
    s.sort();
    let at = |q: f64| s[((s.len() - 1) as f64 * q) as usize].as_secs_f64() * 1e6;
    println!(
        "{label:16} p50 {:7.1} µs   p99 {:7.1} µs   max {:7.1} µs",
        at(0.5),
        at(0.99),
        at(1.0)
    );
}

fn direct_latency() -> Vec<Duration> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new("python3");
    cmd.args(["-c", ECHO]);
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    let mut r = pair.master.try_clone_reader().unwrap();
    let mut w = pair.master.take_writer().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let mut buf = [0u8; 64];
    let out = (0..1000)
        .map(|i| {
            let b = [b'a' + (i % 26) as u8];
            let t = Instant::now();
            w.write_all(&b).unwrap();
            loop {
                let n = r.read(&mut buf).unwrap();
                if buf[..n].contains(&b[0]) {
                    break;
                }
            }
            t.elapsed()
        })
        .collect();
    let _ = child.kill();
    out
}

const TOKEN: &[u8] = b"measure-token-0123456";

fn start_jar(bin: &Path, dir: &Path, name: &str, argv: &[&str]) -> (Child, PathBuf) {
    let socket = dir.join(name);
    let child = Command::new(bin)
        .arg("--socket")
        .arg(&socket)
        .args(["--window", "8388608", "--ring", "8388608", "--"])
        .args(argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = child;
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), TOKEN).unwrap();
    (child, socket)
}

fn finish(mut jar: Child, client: &Client) {
    let _ = client.send(&Msg::Signal { signal: 3 });
    std::thread::sleep(Duration::from_millis(100));
    let _ = jar.kill();
    let _ = jar.wait();
}

fn jar_latency(bin: &Path, dir: &Path) -> Vec<Duration> {
    let (jar, socket) = start_jar(bin, dir, "lat.sock", &["python3", "-c", ECHO]);
    let (mut c, _) = Client::connect(&socket, Role::Owner, Duration::from_secs(5), TOKEN).unwrap();
    c.send(&Msg::Replay { from: 0 }).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    while c.recv(Duration::from_millis(50)).unwrap().is_some() {}
    let out = (0..1000)
        .map(|i| {
            let b = vec![b'a' + (i % 26) as u8];
            let t = Instant::now();
            c.send(&Msg::Input { bytes: b.clone() }).unwrap();
            loop {
                if let Some(Msg::Output { offset, bytes }) = c.recv(Duration::from_secs(2)).unwrap()
                {
                    let _ = c.send(&Msg::Ack {
                        offset: offset + bytes.len() as u64,
                    });
                    if bytes.contains(&b[0]) {
                        break;
                    }
                }
            }
            t.elapsed()
        })
        .collect();
    finish(jar, &c);
    out
}

const FIREHOSE: &str = "head -c 100000000 /dev/zero";

fn direct_throughput() -> f64 {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args(["-c", FIREHOSE]);
    let t = Instant::now();
    let _child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut r = pair.master.try_clone_reader().unwrap();
    let mut buf = vec![0u8; 65536];
    let mut total = 0usize;
    while let Ok(n) = r.read(&mut buf) {
        if n == 0 {
            break;
        }
        total += n;
    }
    total as f64 / t.elapsed().as_secs_f64() / 1e6
}

fn jar_throughput(bin: &Path, dir: &Path) -> f64 {
    let (jar, socket) = start_jar(bin, dir, "tp.sock", &["/bin/sh", "-c", FIREHOSE]);
    let t = Instant::now();
    let (mut c, _) = Client::connect(&socket, Role::Owner, Duration::from_secs(5), TOKEN).unwrap();
    c.send(&Msg::Replay { from: 0 }).unwrap();
    let mut total = 0u64;
    loop {
        match c.recv(Duration::from_secs(5)).unwrap() {
            Some(Msg::Output { offset, bytes }) => {
                total += bytes.len() as u64;
                let _ = c.send(&Msg::Ack {
                    offset: offset + bytes.len() as u64,
                });
            }
            Some(Msg::Exited { .. }) | None => break,
            Some(_) => {}
        }
    }
    let rate = total as f64 / t.elapsed().as_secs_f64() / 1e6;
    finish(jar, &c);
    rate
}

/// A program prints 4 MB with no owner; a new owner replays all of it.
fn replay(bin: &Path, dir: &Path) -> (u64, Duration) {
    let (jar, socket) = start_jar(
        bin,
        dir,
        "rp.sock",
        &[
            "/bin/sh",
            "-c",
            "head -c 4000000 /dev/zero | tr '\\000' r; sleep 30",
        ],
    );
    std::thread::sleep(Duration::from_secs(2));
    let t = Instant::now();
    let (mut c, welcome) =
        Client::connect(&socket, Role::Owner, Duration::from_secs(5), TOKEN).unwrap();
    let Msg::Welcome { head, base, .. } = welcome else {
        unreachable!()
    };
    c.send(&Msg::Replay { from: base }).unwrap();
    let mut got = base;
    while got < head {
        if let Some(Msg::Output { offset, bytes }) = c.recv(Duration::from_secs(5)).unwrap() {
            got = offset + bytes.len() as u64;
            let _ = c.send(&Msg::Ack { offset: got });
        }
    }
    let took = t.elapsed();
    finish(jar, &c);
    (head - base, took)
}

/// 30 jars, each holding an idle `cat`; the resident memory of the jars alone.
fn thirty_jars(bin: &Path, dir: &Path) -> (u64, u64) {
    let jars: Vec<(Child, PathBuf)> = (0..30)
        .map(|i| start_jar(bin, dir, &format!("m{i}.sock"), &["/bin/cat"]))
        .collect();
    let clients: Vec<Client> = jars
        .iter()
        .map(|(_, s)| {
            Client::connect(s, Role::Owner, Duration::from_secs(5), TOKEN)
                .unwrap()
                .0
        })
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let pids: Vec<String> = jars.iter().map(|(c, _)| c.id().to_string()).collect();
    let ps = Command::new("ps")
        .args(["-o", "rss=", "-p", &pids.join(",")])
        .output()
        .unwrap();
    let rss: u64 = String::from_utf8_lossy(&ps.stdout)
        .split_whitespace()
        .filter_map(|n| n.parse::<u64>().ok())
        .sum();
    for ((jar, _), c) in jars.into_iter().zip(&clients) {
        finish(jar, c);
    }
    (rss, 30)
}
