# Turnstile — Operator Guide

## Daily operation

### Starting Turnstile
Open two terminals:

**Terminal 1 — Actions responder:**
```bash
cd responder
export TURNSTILE_RECIPIENT="YOUR_WALLET_PUBKEY"
export TURNSTILE_DEVNET="1"
cargo run --release
```

**Terminal 2 — ZeroClaw daemon:**
```bash
zeroclaw daemon
```

### Creating an event
In Discord or Telegram:

@Turnstile create event "Event Title" capacity N, price X USDC tier-name


### Announcing enrollment

@Turnstile announce enrollment Blink for "Event Title" event id <event_id>


### Checking status

@Turnstile how many spots left for <event_id>


### Changing price tier

@Turnstile activate standard pricing for <event_id>


### Processing a refund

@Turnstile refund attendee <pubkey> for <event_id>

The agent will trigger an approval checkpoint. You must explicitly approve
before any action is taken.

## Configuration updates

### Changing the responder URL (e.g. after ngrok restart)

@Turnstile my new responder URL is https://new-url.ngrok-free.dev


### Switching to mainnet
1. Update `TURNSTILE_DEVNET=0` in your responder startup
2. Update `rpc_url` in config to a mainnet RPC endpoint
3. Ensure your recipient wallet has a USDC token account on mainnet

## Monitoring

- **Dashboard:** `http://127.0.0.1:42617/`
- **Health check:** `curl https://YOUR_RESPONDER/health`
- **Session logs:** Available in the dashboard under Sessions
- **Memory:** Available in the dashboard under Memories

## Troubleshooting

### Bot not responding in Discord
- Check bot is online in server member list
- Verify daemon is running: `zeroclaw daemon`
- Check bot token is valid in Developer Portal

### Responder not reachable
- Verify responder is running: `curl http://localhost:8080/health`
- Verify ngrok is running and URL matches config
- Check `TURNSTILE_RECIPIENT` env var is set

### Payment not confirming
- Check RPC endpoint is reachable
- Check `turnstile:rpc_errors` in memory for error details
- Verify reference key was included in the transaction

## Security reminders

- Never share your bot tokens or API keys
- Never run the responder's admin endpoints on a public port
- Always use a fresh ngrok URL or custom domain for production
- Rotate your bot token if you suspect compromise
- The agent never holds funds — all payments go directly to your wallet