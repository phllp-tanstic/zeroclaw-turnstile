# Skill: Payment Confirmation (Turnstile)

## Purpose
Poll the Solana blockchain to confirm enrollment payments.
This skill is called ONLY by the payment-confirmation SOP cron —
never triggered by chat messages.

## How to confirm a payment

1. Read the pending reference keys from memory:
   memory_read("turnstile:pending_refs")

2. For each reference key, call:
   GET {rpc_url}
   Method: POST
   Body:
   {
     "jsonrpc": "2.0",
     "id": 1,
     "method": "getSignaturesForAddress",
     "params": [
       "{reference_key}",
       { "limit": 5, "commitment": "confirmed" }
     ]
   }

3. Shape the response — extract ONLY:
   - signature (first result)
   - confirmationStatus
   - err (null = success)
   Keep response under 200 tokens. Discard everything else.

4. If err is null and confirmationStatus is "confirmed" or "finalized":
   - Write to memory: memory_write("turnstile:confirmed:{reference_key}", {signature, timestamp})
   - Remove from pending: update memory_read("turnstile:pending_refs")
   - Post confirmation to organizer channel:
     "✅ Payment confirmed for enrollment ref {short_ref}
      Signature: {sig[:8]}...
      Spots remaining: {n}"

5. If not yet confirmed: do nothing, wait for next cron tick.

6. If RPC returns an error:
   - Log: memory_write("turnstile:rpc_errors", {timestamp, error, ref_key})
   - Do NOT mark as confirmed or failed
   - Retry on next cron tick

## Response shaping rule
Raw RPC responses contain many fields the model does not need.
Always extract only the fields listed in step 3 above.
Never pass a raw RPC response into your reasoning context.