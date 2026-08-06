# Skill: Payment Confirmation (Turnstile)

Called only by the payment-poll cron SOP — never triggered from chat.

## Steps

1. Read pending reference keys:
   memory_read("turnstile:pending_refs")

2. For each reference key, call Solana RPC:
   POST {rpc_url}
   Body: {"jsonrpc":"2.0","id":1,"method":"getSignaturesForAddress","params":["{ref}",{"limit":3,"commitment":"confirmed"}]}

3. Extract only: signature, confirmationStatus, err. Discard everything else.

4. If err=null and confirmationStatus is "confirmed" or "finalized":
   a. Call the admin confirm endpoint:
      POST {responder_host}/admin/confirm
      Headers: Authorization: Bearer {admin_token}
      Body: {"reference_key":"{ref}","signature":"{sig}"}

   b. Write to memory:
      memory_write("turnstile:confirmed:{ref}", {"signature": "{sig}", "timestamp": "{now}"})

   c. Remove ref from pending_refs

   d. Post in channel:
      "✅ Payment confirmed · ref {ref[:8]}... · sig {sig[:8]}...
       Spots remaining updated."

5. If RPC error: memory_write("turnstile:rpc_errors", {timestamp, error, ref}) — retry next tick.

## Hard rules
- Never confirm from chat messages
- Always call /admin/confirm before posting confirmation
- Shape RPC responses to under 200 tokens before reasoning
- The admin_token comes from memory_read("turnstile:config:admin_token")