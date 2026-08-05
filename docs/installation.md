# Turnstile — Installation Guide

## Prerequisites

- Windows 10/11, macOS, or Linux
- [Rust](https://rustup.rs/) (for building the Actions responder)
- Git
- A Discord bot token ([discord.com/developers](https://discord.com/developers))
- An OpenAI API key ([platform.openai.com](https://platform.openai.com))
- A Solana RPC endpoint (devnet: `https://api.devnet.solana.com`)
- ngrok or a custom domain for the Actions responder

## Step 1 — Install ZeroClaw

```bash
# Unix
curl -fsSL https://raw.githubusercontent.com/zeroclaw-labs/zeroclaw/master/install.sh | sh

# Windows (PowerShell)
git clone --depth 1 https://github.com/zeroclaw-labs/zeroclaw
cd zeroclaw
.\setup.bat --default
```

Verify:
```bash
zeroclaw --version
# zeroclaw 0.8.4
```

## Step 2 — Clone Turnstile

```bash
git clone https://github.com/YOUR_USERNAME/zeroclaw-turnstile
cd zeroclaw-turnstile
```

## Step 3 — Build the Actions responder

```bash
cd responder
cargo build --release
```

## Step 4 — Configure ZeroClaw

Run quickstart to create your agent:
```bash
zeroclaw quickstart
```

- Model provider: OpenAI (or any supported provider)
- Model: `gpt-4o` or `gpt-4o-mini`
- Memory: `sqlite`
- Channels: Discord (and optionally Telegram)
- Agent alias: `turnstile`

Then copy the example config and edit it:
```bash
cp config/turnstile.example.toml config/turnstile.toml
```

Edit `config/turnstile.toml` with your values:
- `responder_host` — your public ngrok or domain URL
- `recipient_pubkey` — your Solana wallet address
- `discord_announcement_channel` — your Discord channel ID
- `rpc_url` — your Solana RPC endpoint

## Step 5 — Set secrets

```bash
zeroclaw config set providers.models.openai.turnstileopenai.api_key
zeroclaw config set channels.discord.turnstilediscord.bot_token
```

## Step 6 — Create event state

Copy the example state file:
```bash
cp responder/turnstile-state.example.json responder/turnstile-state.json
```

Edit it with your event details.

## Step 7 — Start the responder

```bash
# Set your recipient wallet
export TURNSTILE_RECIPIENT="YOUR_WALLET_PUBKEY"
export TURNSTILE_DEVNET="1"  # set to 0 for mainnet

cd responder
cargo run --release
```

## Step 8 — Expose the responder

```bash
ngrok http 8080
```

Copy the HTTPS URL (e.g. `https://abc123.ngrok-free.dev`).

## Step 9 — Start the daemon

```bash
zeroclaw daemon
```

## Step 10 — Verify

Run the verification script:
```bash
scripts/verify-setup.sh
```

Or manually:
```bash
curl https://YOUR_NGROK_URL/health
curl https://YOUR_NGROK_URL/.well-known/actions.json
curl https://YOUR_NGROK_URL/actions/enroll?event_id=YOUR_EVENT_ID
```

## Step 11 — Tell the agent your config

In Discord or Telegram, send:
@Turnstile my responder is https://YOUR_NGROK_URL and recipient wallet is YOUR_WALLET_PUBKEY

Then create your first event:
@Turnstile create event "My Workshop" capacity 50, price 25 USDC early bird

