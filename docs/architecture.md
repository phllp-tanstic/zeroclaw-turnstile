# Turnstile — Architecture

## Overview

Turnstile is a ZeroClaw agent that runs live-priced event and cohort enrollment
through Solana Blinks, with zero custody and zero backend.

## Components

### 1. ZeroClaw Agent (daemon)
The ZeroClaw agent runs as a daemon process and handles all conversational
logic, scheduling, and channel communication.

- **Channels:** Discord, Telegram
- **Skills:** `turnstile-enroll`, `turnstile-payment`
- **Cron:** `payment-poll` (runs every minute)
- **Memory:** SQLite (roster, config, pending payments)
- **Model:** GPT-4o (via OpenAI API)

### 2. Turnstile Actions Responder
A small, deterministic Rust HTTP service that serves the Solana Actions
(Blinks) endpoints. It is deliberately separate from the ZeroClaw agent
because wallet clients expect sub-second, deterministic JSON responses —
routing through an LLM reasoning loop would be the wrong shape for this job.

**Endpoints:**
- `GET /.well-known/actions.json` — declares this domain hosts Solana Actions
- `GET /actions/enroll` — returns live event preview (spots, price tier)
- `POST /actions/enroll` — returns unsigned USDC transfer transaction
- `GET /health` — health check

**The responder:**
- Never holds a private key
- Never signs a transaction
- Reads event state from a shared JSON file written by the agent
- Returns transactions with zero blockhash — wallets replace on sign

### 3. ngrok Tunnel (development) / Custom Domain (production)
Exposes the Actions responder to the public internet so wallet clients
and Blinks-aware platforms (Discord, X) can reach it.

## Architecture Diagram

Organizer (Discord/Telegram)
│
▼
ZeroClaw Agent (daemon)
├── Skills (enroll-event, payment-confirm)
├── SOP Cron (payment-poll every 60s)
├── Memory (SQLite: roster, config, pending refs)
└── Channels (Discord, Telegram)
│
├── Writes event state ──────────────────────► turnstile-state.json
│ │
│ Turnstile Actions Responder
│ (deterministic Rust HTTP)
│ │
│ GET /actions/enroll
│ POST /actions/enroll
│ │
│ Attendee Wallet
│ (signs & broadcasts)
│ │
└── Polls getSignaturesForAddress ◄────────── Solana RPC
(every 60s)

## Layering rationale

The bounty brief scores "correct layering" explicitly. Here is why each
component lives where it does:

| Component | Layer | Reason |
|---|---|---|
| Event creation, announcements, confirmations | ZeroClaw agent (T1) | Conversational, scheduled, channel-aware — exactly what ZeroClaw is for |
| Blink GET/POST endpoint | Standalone Rust service | Wallets expect deterministic sub-second JSON; LLM loop is wrong shape |
| Transaction signing | Never happens | Agent holds no keys; attendee's wallet signs |
| Refund execution | Never automatic | Always routed through approval checkpoint |

## Custody tier: T1

The agent never holds a private key and never signs a transaction.
The one on-chain write path (USDC transfer) is constructed by the Actions
responder and signed exclusively by the attendee's own wallet.

Secrets held by the system:
- Solana RPC endpoint URL (operator-supplied)
- OpenAI API key (encrypted in ZeroClaw keyring)
- Discord/Telegram bot tokens (encrypted in ZeroClaw keyring)

No private keys. No custodial processor. No third-party database.