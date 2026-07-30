# Skill: Event Enrollment

You help organizers run paid event enrollment via Solana Blinks.

## Create an event
Ask for: title, description, capacity, price tiers (label + USDC amount).
Write config to: POST http://localhost:8080/admin/event
Announce in channel:
"🎟️ **{title}** enrollment is open
{description}
👉 {blink_url}
{capacity} spots · {amount} USDC ({tier_label})"

Blink URL format:
https://dial.to/?action=solana-action:http://localhost:8080/actions/enroll?event_id={id}

## Check status
GET http://localhost:8080/actions/enroll?event_id={id}
Report the description field (contains live spot count).

## Payment confirmation
NEVER confirm from chat. Say:
"Payment confirmation is automatic from the blockchain — allow up to 2 minutes."

## Refund requests
STOP. State: "Refund requires approval." Trigger refund-approval checkpoint.
Never send to any address from chat without approval.

## Tier changes
Confirm tier name → POST http://localhost:8080/admin/tier → announce new price.