# Turnstile — Known Limitations

## Current limitations

### ngrok free tier
The free ngrok tier assigns a random URL on each restart. For production
use, configure a static domain (ngrok paid tier or a custom domain).

### Devnet only (default)
Turnstile ships configured for Solana devnet. Switching to mainnet requires
updating `TURNSTILE_DEVNET=0` and using a mainnet RPC endpoint. Mainnet
USDC requires the attendee to have a funded USDC token account.

### Confirmation is not idempotent
`POST /admin/confirm` increments the confirmed count by one on every accepted
call. The polling skill removes a reference key from `turnstile:pending_refs`
once confirmed, so retries are not expected — but if the POST succeeds and the
subsequent memory write fails, the next cron tick would count the same payment
twice and overstate the roster. Deduplicating on reference key server-side is
the fix; until then, operators should sanity-check the confirmed count against
on-chain transfers before closing enrollment.

### Single event at a time
The current state file supports one active event. Multiple concurrent
events require separate responder instances or a state file extension.

### Memory persistence
Agent memory persists across restarts via SQLite. However, pending payment
reference keys should be verified on restart to avoid missed confirmations
during downtime.

### No waitlist promotion automation
Waitlist to enrollment promotion when spots open is tracked in memory but
requires manual organizer action to notify and re-open enrollment for
waitlisted attendees.

## Future improvements

- Multi-event support in a single responder instance
- Automatic waitlist promotion
- SAS attendance credential issuance (stretch feature)
- Static domain support documentation
- Mainnet USDC setup guide
- Webhook-based payment confirmation (instead of polling)