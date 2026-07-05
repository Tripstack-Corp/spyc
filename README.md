# spyc apt repository

Signed Debian/Ubuntu packages for
[spyc](https://github.com/Tripstack-Corp/spyc) — a keyboard-driven,
MCP-native terminal file commander. Served via GitHub Pages at
<https://tripstack-corp.github.io/spyc>.

## Install

```sh
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://tripstack-corp.github.io/spyc/KEY.gpg \
  | sudo tee /etc/apt/keyrings/spyc.asc >/dev/null
echo "deb [signed-by=/etc/apt/keyrings/spyc.asc] https://tripstack-corp.github.io/spyc ./" \
  | sudo tee /etc/apt/sources.list.d/spyc.list >/dev/null
sudo apt update
sudo apt install spyc
```

`apt upgrade` picks up new releases from then on. Both `amd64` and `arm64`
are published. Packages are built and signed automatically on each stable
spyc release by the `apt.yml` workflow. `KEY.gpg` is the repository signing
key's public half (fingerprint `6039 1763 40F9 215E 2756 AFE9 D57E 3D74 C8FC 6618`).

This branch is machine-managed — the source lives on `main`.
