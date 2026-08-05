# Turnstile — Workflow

## Complete enrollment flow

### Trigger
Organizer sends a message in Discord or Telegram:
@Turnstile create event "Solana Builders Workshop" capacity 50, price 25 USDC early bird

### Step 1 — Event creation
Agent parses the event details and stores them in memory:
- `turnstile:event:evt_001` — event config
- `turnstile:config:responder_host` — responder URL (asked once, stored)

### Step 2 — Blink announcement
Agent constructs the Blink URL from memory and posts in channel:
🎟️ Solana Builders Workshop — Enrollment is open!
A hands-on workshop for developers building on Solana.

💰 25 USDC (Early Bird) · 🪑 50 spots

👉 https://dial.to/?action=solana-action:https://your-domain.com/actions/enroll?event_id=evt_001

### Step 3 — Attendee taps Blink
1. Wallet client fetches `GET /actions/enroll?event_id=evt_001`
2. Responder returns live preview: current price, spots remaining
3. Attendee taps "Enroll — 25 USDC"
4. Wallet calls `POST /actions/enroll` with attendee's public key
5. Responder returns unsigned USDC transfer transaction (zero blockhash)
6. Wallet fetches fresh blockhash, signs, and broadcasts to Solana

### Step 4 — Payment confirmation (automated)
Every 60 seconds, the payment-poll cron SOP runs:
1. Reads pending reference keys from `turnstile:pending_refs`
2. Calls `getSignaturesForAddress` on the RPC for each reference key
3. If confirmed: writes to `turnstile:confirmed:<ref>`, posts confirmation
4. If RPC error: logs to `turnstile:rpc_errors`, retries next tick

### Step 5 — Confirmation message
Agent posts in channel:
✅ Payment confirmed for enrollment ref 8dEB36Ww...
Signature: 4xK9mP2r...
Spots remaining: 49

### Step 6 — Refund (if needed)
Organizer requests a refund:

@Turnstile refund attendee <pubkey>

Agent triggers the approval checkpoint SOP:
- Organizer must explicitly approve
- Agent verifies refund address matches original payer
- Organizer signs the refund transaction from their own wallet
- Agent never executes the refund automatically

## Price tier changes
@Turnstile activate standard pricing

Agent confirms, updates state, announces new price in channel.

## Capacity management

When capacity hits zero:
- GET preview shows "Join waitlist" instead of "Enroll"
- POST returns a waitlist reference instead of a payment transaction
- Agent tracks waitlist in memory and notifies when spots open