import json
with open('C:/dev/zeroclaw-turnstile/architecture/config-schema.json', encoding='utf-8') as f:
    s = json.load(f)
defs = s.get('$defs', {})
dc = defs.get('DiscordConfig', {})
props = dc.get('properties', {})
for k, v in props.items():
    desc = v.get('description', '')
    default = v.get('default', 'NO_DEFAULT')
    print(f"{k}: default={default} | {desc[:80]}")
