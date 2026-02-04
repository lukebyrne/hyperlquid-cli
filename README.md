# Hyperliquid CLI (Rust)

A command-line interface for [Hyperliquid DEX](https://hyperliquid.xyz/) — a direct 1:1 Rust port of the npm package [`hyperliquid-cli`](https://www.npmjs.com/package/hyperliquid-cli).

Features a beautiful terminal UI with real-time watch modes and an optional background caching server.

> Note: this repo installs `rhl` / `rhl-server` (to avoid clashing with the npm `hl` binary). It uses the same local storage directory (`~/.hl/`) as the JS CLI.

## Installation

Prereqs:

- Rust toolchain (install via `rustup`)
- Linux only: you may need OpenSSL + `pkg-config` (for `native-tls`)

```bash
cargo install hyperliquid-cli
```

This installs two binaries:

- `rhl`
- `rhl-server`

Install from Git:

```bash
cargo install --git <REPO_URL> --locked
```

Install from this repo (local):

```bash
cargo install --path .
```

If `rhl` isn’t found after installing, make sure `~/.cargo/bin` is on your `PATH`.

## Features

- **Multi-Account Management** - Store and manage multiple accounts locally with SQLite
- **Real-Time Monitoring** - WebSocket-powered live updates for positions, orders, balances, and prices
- **Beautiful Terminal UI** - Color-coded PnL, depth visualization, and interactive tables
- **Trading Support** - Place limit and market orders
- **Scripting Friendly** - JSON output mode for automation and scripting
- **Testnet Support** - Seamless switching between mainnet and testnet

## Global Options

| Option | Description |
|--------|-------------|
| `--json` | Output in JSON format |
| `--testnet` | Use testnet instead of mainnet |
| `-V, --version` | Show version number |
| `-h, --help` | Show help |

---

## Account Management

Manage multiple accounts locally. Accounts are stored in a SQLite database at `~/.hl/hl.db`.

### Add Account

Interactive wizard to add a new account:

```bash
rhl account add
```

- Import API wallets from Hyperliquid for trading
- Add read-only accounts for monitoring only
- Set account aliases for easy identification
- Choose a default account for commands

### List Accounts

```bash
rhl account ls
```

Shows all configured accounts with alias, address, type, and default status.

### Set Default Account

```bash
rhl account set-default <alias>
```

Sets which account to use by default.

### Remove Account

```bash
rhl account remove <alias>
```

Interactively remove an account from local storage.

---

## Balance & Portfolio Monitoring

View account balances and portfolio with optional real-time watch mode.

### Get Balances

```bash
# Spot + perpetuals balances
rhl account balances

# Watch mode - real-time updates
rhl account balances -w

# Specific address
rhl account balances --user 0x...
```

Shows spot token balances (total, hold, available) and perpetuals USD balance.

### Get Full Portfolio

```bash
# Positions + spot balances combined
rhl account portfolio

# Watch mode
rhl account portfolio -w
```

Combined view of all positions and spot balances in a single display.

---

## Position Monitoring

View and monitor perpetual positions.

### Get Positions

```bash
# One-time fetch
rhl account positions

# Watch mode - real-time updates with colored PnL
rhl account positions -w

# Specific address
rhl account positions --user 0x...
```

Displays: coin, size, entry price, position value, unrealized PnL, leverage, and liquidation price.

---

## Order Management

View, place, and cancel orders (requires an API wallet account for trading).

### View Open Orders

```bash
rhl account orders

# Watch mode - real-time order updates
rhl account orders -w

# Specific address
rhl account orders --user 0x...
```

Or use:

```bash
rhl order ls
```

### Place Limit Order

```bash
rhl order limit <side> <size> <coin> <price>

# Examples
rhl order limit buy 0.001 BTC 50000
rhl order limit sell 0.1 ETH 3500 --tif Gtc
rhl order limit long 1 SOL 100 --reduce-only
```

| Option | Description |
|--------|-------------|
| `--tif <tif>` | Time-in-force: `Gtc` (default), `Ioc`, `Alo` |
| `--reduce-only` | Reduce-only order |

### Place Market Order

```bash
rhl order market <side> <size> <coin>

# Examples
rhl order market buy 0.001 BTC
rhl order market sell 0.1 ETH --slippage 0.5
```

| Option | Description |
|--------|-------------|
| `--slippage <pct>` | Slippage percentage (overrides config) |
| `--reduce-only` | Reduce-only order |

### Configure Order Defaults

```bash
# View current configuration
rhl order configure

# Set default slippage for market orders
rhl order configure --slippage 0.5
```

### Cancel Order

```bash
# Cancel specific order
rhl order cancel <oid>

# Interactive selection from open orders
rhl order cancel
```

### Cancel All Orders

```bash
# Cancel all open orders
rhl order cancel-all

# Cancel all orders for a specific coin
rhl order cancel-all --coin BTC

# Skip confirmation
rhl order cancel-all -y
```

### Set Leverage

```bash
# Cross margin (default)
rhl order set-leverage <coin> <leverage>
rhl order set-leverage BTC 10

# Isolated margin
rhl order set-leverage BTC 10 --isolated

# Explicit cross margin
rhl order set-leverage ETH 5 --cross
```

---

## Market Information

View market data without authentication.

### List All Markets

```bash
# List all perpetual and spot markets
rhl markets ls

# Watch mode
rhl markets ls -w
```

This command requires the background server (see below).

### Get All Prices

(Hidden/experimental command.)

```bash
rhl markets prices

# Watch mode
rhl markets prices -w
```

---

## Asset Information

View asset-specific data with optional watch mode.

### Get Price

```bash
# One-time fetch
rhl asset price BTC

# Watch mode - real-time price updates
rhl asset price BTC -w
```

### Get Order Book

```bash
# One-time fetch with depth visualization
rhl asset book BTC

# Watch mode - real-time order book
rhl asset book ETH -w
```

Shows top bid/ask levels with cumulative depth bars and spread calculation.

### Get Leverage Info

```bash
# One-time fetch
rhl asset leverage BTC

# Watch mode
rhl asset leverage BTC -w

# Specific address
rhl asset leverage BTC --user 0x...
```

---

## Referral System

(Hidden/experimental commands.)

### Set Referral Code

```bash
rhl referral set <code>
```

### Get Referral Status

```bash
rhl referral status
```

---

## Background Server

Optional background server for caching market data and faster queries (required for `rhl markets ls`).

### Start Server

```bash
rhl server start
```

### Stop Server

```bash
rhl server stop
```

### Check Status

```bash
rhl server status
```

Shows server status, WebSocket connection state, uptime, and cache status.

---

## Upgrade

```bash
rhl upgrade
rhl upgrade --json
```

Checks the npm registry for the latest `hyperliquid-cli` version and shows upgrade instructions for this Rust port.

---

## Examples

### Testnet Trading

```bash
# Check positions on testnet
rhl --testnet account positions

# Place a testnet order
rhl --testnet order limit buy 0.001 BTC 50000
```

### Real-Time Monitoring

```bash
# Watch positions with live PnL
rhl account positions -w

# Watch order book with depth visualization
rhl asset book BTC -w

# Watch specific asset price
rhl asset price ETH -w
```

### Scripting with JSON Output

```bash
# Get BTC price
BTC_PRICE=$(rhl asset price BTC --json | jq -r '.price')
echo "BTC: $BTC_PRICE"

# Get all positions as JSON
rhl account positions --json | jq '.positions[] | {coin, size, pnl: .unrealizedPnl}'

# Check open orders
rhl account orders --json | jq '.[] | select(.coin == "BTC")'
```

### Automated Trading

```bash
#!/bin/bash
# Simple limit order script

COIN="BTC"
SIDE="buy"
SIZE="0.001"
PRICE="85000"

echo "Placing $SIDE order for $SIZE $COIN @ $PRICE"
rhl order limit $SIDE $SIZE $COIN $PRICE --json
```

---

## Configuration

### Environment variables

- `HYPERLIQUID_PRIVATE_KEY=0x...` (optional; used if no default account is set)
- `HYPERLIQUID_WALLET_ADDRESS=0x...` (optional; derived from key if omitted)
- `HYPERLIQUID_CLI_DIR=/path/to/dir` (optional; overrides `~/.hl`)

### Local storage

All files live under `~/.hl/`:

- `~/.hl/hl.db` (accounts SQLite DB)
- `~/.hl/order-config.json` (market-order slippage default)
- `~/.hl/server.sock`, `~/.hl/server.pid`, `~/.hl/server.json`, `~/.hl/server.log` (background server)
