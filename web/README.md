# Turnstile Web

The enrollment page for Turnstile. A single static HTML file — no build step, no dependencies to install.

## Usage

Open in any browser with query parameters:
index.html?responder=https://your-responder.ngrok-free.dev&event_id=evt_001


## Deploy to Vercel (one command)

```bash
npx vercel web/
```

Vercel gives you a permanent HTTPS URL. Share it with attendees.

## How organizers use it

The Turnstile agent posts this URL in Discord when announcing an event:

https://your-turnstile-app.vercel.app/?responder=https://your-responder.dev&event_id=evt_001


Anyone who clicks it sees the live enrollment card — spots remaining, current price tier, wallet pay button. No accounts. No setup. No leaving the chat.