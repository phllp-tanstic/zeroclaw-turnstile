# Skill: Payment Confirmation

Called only by cron — never from chat.

## Steps
1. Read: memory_read("turnstile:pending_refs")
2. For each ref key POST to RPC:
   {"jsonrpc":"2.0","id":1,"method":"getSignaturesForAddress","params":["{ref}",{"limit":3,"commitment":"confirmed"}]}
3. Extract only: signature, confirmationStatus, err. Discard everything else.
4. If err=null and status=confirmed/finalized:
   - memory_write("turnstile:confirmed:{ref}", {signature, timestamp})
   - Remove from pending_refs
   - Post: "✅ Payment confirmed · ref {ref[:8]}... · sig {sig[:8]}..."
5. If RPC error: memory_write("turnstile:rpc_errors", {timestamp, error}) — do not mark confirmed.