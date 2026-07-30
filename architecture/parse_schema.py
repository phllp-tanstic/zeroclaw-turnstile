import json
with open('C:/dev/zeroclaw-turnstile/architecture/config-schema.json', encoding='utf-8') as f:
    s = json.load(f)
defs = s.get('$defs', {})
print(json.dumps(defs.get('IdentityConfig', {}), indent=2))
