import json
with open('C:/dev/zeroclaw-turnstile/architecture/config-schema.json', encoding='utf-8') as f:
    s = json.load(f)
defs = s.get('$defs', {})
agent = defs.get('AliasedAgentConfig', {})
props = list(agent.get('properties', {}).keys())
print(props)
