# TorPC

Privacy-focused Ethereum JSON-RPC proxy served as a Tor hidden service, with
optional MEV-protected sends via Flashbots.

There are two binaries in this workspace:

- **`torpc`** — the daemon. Runs alongside a Geth node, exposes a Tor
  hidden service that filters JSON-RPC, rate-limits, and optionally signs
  bundles for Flashbots. Operators run this.
- **`torpc-proxy`** — the local client proxy. A wallet user runs this on
  their own machine; it listens on `127.0.0.1:8545`, forwards each request
  through Tor to the configured `.onion`, and returns the response.

> **Status**: working but not yet release-distributed. Build from source.

---

## Roles

| Role | Goal | Path |
|---|---|---|
| Operator | Run a Tor-fronted RPC, optionally protect sends via Flashbots | [Operator Setup](#operator-setup) |
| Wallet user | Route a wallet through someone else's TorPC `.onion` | [Wallet User Setup](#wallet-user-setup) |
| Developer | Hack on TorPC | [Development](#development) |

---

## Operator Setup

### Prerequisites

- **Rust** 1.70+ (`rustup default stable`)
- **Geth** 1.10+ (or any Ethereum execution client speaking JSON-RPC)
- **Tor** 0.4.5+ (`apt install tor`, `brew install tor`, etc.)
- Linux/macOS. Windows works under WSL2.

### Build

```bash
git clone https://github.com/CPerezz/torpc.git
cd torpc
cargo build --release --bin torpc
```

The daemon binary lands in `target/release/torpc`.

### Configure

The daemon reads its config from environment variables. Every var (with
defaults and effects) is documented in [`.env.example`](.env.example) — copy
it to `.env` and edit in place; `dotenvy` auto-loads it at startup.

The most common knobs:

```dotenv
GETH_URL=http://127.0.0.1:8545          # Upstream Ethereum node
BIND_ADDR=127.0.0.1:8080                # Tor hidden service forwards here
RUST_LOG=info                           # Use `debug` to see the .onion address

# Optional: Flashbots MEV protection. Without a signing key, sendBundle
# returns JSON-RPC -32004 instead of silently faking a bundle hash.
# FLASHBOTS_SIGNING_KEY=<64-hex-chars-no-0x>
# FLASHBOTS_RELAY_URL=https://relay.flashbots.net
```

### Tor

The hidden service config lives at [`configs/torrc`](configs/torrc). The
daemon parses it on startup and **refuses to start** if it disables anonymity
(`HiddenServiceSingleHopMode 1` or `HiddenServiceNonAnonymousMode 1`) — set
`TORPC_ALLOW_NON_ANONYMOUS=1` to override (CI/benchmarks only; never in
production).

```bash
# First-time setup: generate the hidden service identity
mkdir -p data/tor/torpc && chmod 700 data/tor/torpc
tor -f configs/torrc
# After Tor bootstraps (~30-60s):
cat data/tor/torpc/hostname
# 3g2upl4pq6kufc4m7v5jzvfncqvhv5bak6d2nprpz7hgbc6jvdnq5jqd.onion
```

The daemon does **not** print the .onion at INFO log level (it leaks into
syslog/log shippers). Use `RUST_LOG=debug` or `cat` the hostname file.

### Run

For local development, use the bundled scripts that start everything:

```bash
./scripts/start-all-dev.sh    # Geth --dev + Tor + daemon
./scripts/stop-all.sh         # Graceful shutdown
```

For production, ship via systemd. Templates live in
[`deploy/systemd-example/`](deploy/systemd-example/):

```bash
# Render and install (asks for the deploy paths):
sudo deploy/systemd-example/install-systemd.sh
sudo systemctl daemon-reload
sudo systemctl enable --now torpc-tor torpc-daemon

# Logs and status
sudo journalctl -u torpc-daemon -f
sudo systemctl status torpc-daemon
```

The unit ships with `NoNewPrivileges`, `ProtectSystem=strict`, `PrivateTmp`,
and `RestrictAddressFamilies` — read each `.service.template` before
installing.

### Observability

```bash
curl http://127.0.0.1:8080/metrics | jq
# {
#   "security_metrics": {
#     "blocked_requests_total": 0,
#     "invalid_methods": 0,
#     "rate_limit_hits": 0,
#     ...
#   },
#   "circuits": { "geth": "closed", "mev_relay": "disabled" }
# }

curl http://127.0.0.1:8080/health
# { "status": "ok", "uptime_seconds": 1234, ... }
```

Both endpoints are reachable through the Tor hidden service today; a future
phase will move admin endpoints onto a separate localhost-only listener.

---

## Wallet User Setup

> **Heads up**: there are no pre-built binaries yet. You'll need a Rust
> toolchain to build the client proxy.

### Build

```bash
git clone https://github.com/CPerezz/torpc.git
cd torpc
cargo build --release -p torpc-proxy-cli
# Binary at: target/release/torpc-proxy
```

### Configure

You need:
- A running Tor daemon on your machine (`brew install tor && tor &` or your
  distro's package — Tor Browser counts but uses port `9150` instead of `9050`).
- The operator's `.onion` address.

Create `torpc-proxy.toml` next to the binary (or pass `--config /path/to.toml`):

```toml
listen_addr   = "127.0.0.1:8545"
tor_proxy     = "127.0.0.1:9050"        # 9150 if you're using Tor Browser
onion_endpoint = "your-operator.onion:80"
```

### Run

```bash
./target/release/torpc-proxy start --config torpc-proxy.toml
```

The proxy strips wallet-fingerprinting headers (User-Agent, Origin, Cookie,
Authorization, Sec-CH-UA-*, etc.) before forwarding through Tor, replacing
them with a synthetic `User-Agent: torpc-proxy/<version>`. This is the
point of the proxy — without it, your MetaMask/Coinbase/Rabby UA gives
you up on every request.

### Point your wallet

In MetaMask / Coinbase Wallet / Rabby / Trust Wallet:
- Add a custom RPC network
- RPC URL: `http://127.0.0.1:8545`
- Chain ID: `1` (Ethereum mainnet) — or whatever the operator's upstream
  is configured for
- Currency Symbol: `ETH`

---

## Configuration reference

[`.env.example`](.env.example) documents every variable the daemon reads.
Highlights:

| Variable | Default | Effect |
|---|---|---|
| `GETH_URL` | `http://127.0.0.1:8545` | Upstream JSON-RPC |
| `BIND_ADDR` | `127.0.0.1:8080` | Daemon listener (the hidden service forwards here) |
| `RUST_LOG` | `info` | tracing-subscriber filter |
| `MAX_REQUEST_SIZE` | `1048576` | Body-size cap (1 MiB) |
| `RATE_LIMIT_REQUESTS` | `100` | Per-(IP, source-port) bucket per window |
| `RATE_LIMIT_WINDOW` | `60` | Window in seconds |
| `WRITE_RATE_LIMIT_REQUESTS` | `10` | Tighter bucket for `eth_sendRawTransaction`/`eth_sendBundle` |
| `MAX_CONCURRENT_CONNECTIONS` | `256` | Router-wide in-flight cap |
| `STRICT_SECURITY_HEADERS` | `true` | When `true`, no CORS. Default. |
| `FLASHBOTS_SIGNING_KEY` | unset | Enables MEV protection when set |
| `FLASHBOTS_RELAY_URL` | `https://relay.flashbots.net` | Mainnet by default |
| `TORPC_ALLOW_NON_ANONYMOUS` | unset | Override anonymity check (CI only) |

---

## Threat model

**This protects against**:
- Network observers (ISP, VPN provider, RPC operator) seeing wallet→RPC
  request contents and metadata. Tor circuits hide the user's IP.
- The upstream RPC operator fingerprinting users via wallet-specific
  headers (User-Agent, Origin, Sec-CH-UA-*) — the client proxy strips these.
- The upstream RPC operator linking transactions to a specific wallet via
  HTTP headers — same mechanism.
- Method-level abuse (rate limits + a strict allow-list of JSON-RPC methods
  blocks `eth_accounts`, `personal_*`, `admin_*`, etc.).
- Front-running for MEV-protected sends — `eth_sendRawTransaction` on
  `/rpc/flashbots` routes to Flashbots with EIP-191 auth.

**This does NOT protect against**:
- A malicious daemon operator. They see your raw JSON-RPC requests
  (including transaction contents). The trust boundary is the operator,
  same as any RPC.
- Traffic-correlation attacks against Tor itself (a global passive
  adversary, etc.).
- A malicious wallet (the wallet sees keys and signs txs locally).
- Smart-contract MEV (sandwich attacks at the protocol level — Flashbots
  helps with sequencing, not contract-level exposure).
- Side channels in the operator's environment (Geth logs, OS journal, etc.)
  if the operator doesn't lock those down.

The daemon goes to some lengths to avoid leaking operator identity to
anonymous Tor visitors:
- No `Server` or `X-Service` response header
- Geth error bodies (which include version strings) are not forwarded
- Per-tx routing decisions and bundle hashes are below INFO log level
- The .onion address is below INFO log level
- Strict CSP, no CORS by default

---

## Development

### Layout

```
torpc/
├── src/                              # Daemon
│   ├── main.rs, app.rs, proxy.rs, ...
│   └── mev/                          # Flashbots auth + retry + circuit breaker
├── torpc-proxy/                      # Client workspace members
│   ├── torpc-proxy-core/
│   ├── torpc-proxy-cli/
│   └── torpc-proxy-gui/              # Tauri desktop UI
├── tests/                            # Daemon integration tests
├── static/                           # Operator-facing wallet onboarding UI
├── configs/torrc                     # Tor hidden service config
├── deploy/systemd-example/           # Templated systemd units
└── scripts/                          # Dev convenience scripts
```

### Test

```bash
make test                # Fast suite, no daemons. ~140 tests in <30s.
make test-with-services  # Brings up Geth + Tor + daemon, runs the whole set.
```

CI runs `make test`, `cargo fmt --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, and `cargo audit` (signal-only for now).

### Local dev loop

```bash
# In one terminal:
./scripts/start-all-dev.sh

# In another:
curl -X POST http://localhost:8080/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Through Tor (after Tor bootstraps):
torsocks curl -X POST http://$(cat data/tor/torpc/hostname)/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

The static UI is reachable at <http://localhost:8080> for wallet
onboarding instructions and a JSON-RPC test panel.

---

## License

MIT — see [LICENSE](LICENSE).

Built with 🦀 Rust and 🧅 Tor.
