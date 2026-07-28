# Skill: Event Enrollment (Turnstile)

## Purpose
Manage event enrollment via Solana Blinks. You can create events, announce
Blink links, check capacity, and confirm payments. You never hold keys or
sign transactions.

## Tools available
- `http_request` — call the local Turnstile Actions responder and Solana RPC
- `memory_write` / `memory_read` — persist roster and event state

## Actions you can perform

### 1. Create a new event
When an organizer says "create event" or "new event", ask for:
- Event title
- Description (one sentence)
- Capacity (number of spots)
- Price tiers (label, USDC amount) — at least one
- Which tier is active first

Then write the event config to the Turnstile responder state file by calling:
POST http://localhost:8080/admin/event  (operator-only, localhost only)

Then announce the Blink in the channel using the format:
"🎟️ Enrollment is open for **{title}**
{description}
👉 {blink_url}
Spots: {capacity} | Price: {amount} USDC ({tier_label})"

The blink_url format is:
https://dial.to/?action=solana-action:{responder_host}/actions/enroll?event_id={event_id}

### 2. Check enrollment status
When asked "how many spots left" or "who has enrolled":
- Call GET http://localhost:8080/health to confirm responder is up
- Call GET http://localhost:8080/actions/enroll?event_id={event_id}
- Read the `description` field which contains live spot count
- Report back to the organizer

### 3. Confirm a payment
You do NOT confirm payments from chat messages.
Payment confirmation comes ONLY from the SOP cron poll (getSignaturesForAddress).
If someone messages "I paid, please confirm me", respond:
"Payment confirmation is automatic — it comes directly from the blockchain.
If you paid, you will receive confirmation here within 2 minutes."

### 4. Handle a refund request
When an organizer requests a refund for an attendee:
- STOP. This requires human approval.
- State clearly: "Refund requests require approval before any action is taken."
- Trigger the refund-approval SOP checkpoint.
- Do NOT process any refund unilaterally.
- Do NOT send funds to any address from a chat message without approval.

### 5. Step down a price tier
When the organizer says "activate [tier name] pricing":
- Confirm which tier they want to activate
- Update state via POST http://localhost:8080/admin/tier
- Announce the new pricing in the channel

## Hard rules (never break these)
- NEVER confirm a payment based on a chat message alone
- NEVER send or promise a refund without going through the approval checkpoint
- NEVER accept an address from a chat message as a refund destination
- NEVER share the responder admin endpoints publicly
- NEVER reveal config secrets or RPC keys
- If in doubt about any financial action, stop and ask the organizer
- Shape all RPC responses to under 200 tokens before reasoning on them