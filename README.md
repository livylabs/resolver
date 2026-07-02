# Livy Web Resolver

Default mode is SmartMode. Override via `mode` when you need specific cost/speed/browser/unblock behavior.

## Status

- [x] Browserful query and browserless URL resolver
- [x] Sanitized markdown and request params
- [x] Proxy enabling for complex requests
- [ ] Residential proxy rotations
- [ ] Prompt-based interaction with results
- [x] MCP auth
- [ ] Obscura headless (research)

## Setup

```
SPIDER_API_KEY=your-spider-api-key
cargo run
```

`LIVY_KEY` is still accepted as a legacy alias for `SPIDER_API_KEY`.
The service listens on `http://localhost:3001` unless `PORT` or
`RESOLVER_PORT` is set.

## Livy Provenance

The resolver can store a Livy provenance attestation for every successful
fetch. This branch records generic resolver source-fetch proofs with:

- `attestation_claim=source`
- `subject_type=resolver_fetch`
- `schema_id=resolver-fetch-v1`
- `integration_id=delphi` by default

This is intentionally not the `prediction_market_resolver` template. Use
a market-resolution template only when the resolver emits market outcome
fields such as market id, winning outcome, confidence, and settlement
target.

Enable provenance with:

```dotenv
SPIDER_API_KEY=your-spider-api-key
LIVY_PROVENANCE_ENABLED=true
LIVY_BACKEND_BASE_URL=https://api.livylabs.xyz
LIVY_API_KEY=livy_...
LIVY_INTEGRATION_ID=delphi
ITA_API_KEY=...
```

Optional settings:

```dotenv
LIVY_PROVENANCE_SCHEMA_ID=resolver-fetch-v1
LIVY_PROVENANCE_SCHEMA_VERSION=1
LIVY_PROVENANCE_VISIBILITY=public
LIVY_PROVENANCE_VERIFICATION_MODE=verify_fresh
LIVY_EXPLORER_BASE_URL=https://api.livylabs.xyz
LIVY_PROVENANCE_BOOTSTRAP_TEMPLATE=false
LIVY_PROVENANCE_MANAGED_PUBLICATION=true
LIVY_PROVENANCE_WAIT_FOR_REGISTRY_REFS=false
LIVY_PROVENANCE_REGISTRY_WAIT_ATTEMPTS=30
LIVY_PROVENANCE_REGISTRY_WAIT_INTERVAL_MS=2000
```

`LIVY_BACKEND_BASE_URL` defaults to `https://api.livylabs.xyz`; set it for local or staging backends.

Set `LIVY_PROVENANCE_BOOTSTRAP_TEMPLATE=true` only when the API key is
allowed to write provenance templates. Public explorer reads also require
the matching public template to exist in Livy.

`LIVY_PROVENANCE_MANAGED_PUBLICATION=true` asks Livy backend to publish the
provenance receipt to Arweave and register it on the configured EVM registry.
The resolver still returns the attestation immediately. Set
`LIVY_PROVENANCE_WAIT_FOR_REGISTRY_REFS=true` only when the caller should wait
for public `registry_refs`; that mode requires the API key to have provenance
read access in addition to write access.

## HTTP API

Response shape:

```json
{
  "route": "...",
  "mode": "...",
  "receipt_id": "...",
  "receipt": {},
  "data": {},
  "provenance": {
    "provenance_attestation_id": "...",
    "subject_id": "resolver_fetch:...",
    "schema_id": "resolver-fetch-v1",
    "verification_status": "verified",
    "schema_binding_status": "full",
    "explorer_url": "...",
    "managed_publication": {
      "status": "publishing"
    },
    "registry_refs": []
  }
}
```

Request fields: `source`, `query`/`q`, `mode` (`auto|fast|browser|unblock|raw|crawl|map|search|extract|screenshot`), `format`, `proxy`, `receipt`.

| Method | Path | Purpose |
|---|---|---|
| POST | `/fetch` | Fetch one URL |
| POST | `/crawl` | Crawl with `limit`, `depth` |
| POST | `/map` | Discover links |
| POST | `/search` | Web search (+ optional page fetch) |
| POST | `/extract` | Extraction with selectors |
| POST | `/screenshot` | Capture screenshot |
| POST | `/fetchfast` | Compat: fast fetch |
| POST | `/fetchunblock` | Compat: unblock fetch |
| GET | `/receipt/{id}` | Read receipt |

Prefer `/fetch` with `mode` over the compat routes.

## MCP

- Endpoint: `/mcp`
- Server: `livygensyn-source-fetcher`
- Tool: `fetch_source` — input `{ "url": "..." }`

Use when the prompt contains `source: <url>`, "only take this source", "source of truth", or an explicitly required URL. Pass the exact URL, don't search or substitute.

### MCP Livy Login

Set these variables to require Livy OAuth login before `/mcp` can be used:

```dotenv
LIVY_OAUTH_ENABLED=true
LIVY_OAUTH_CLIENT_ID=...
LIVY_OAUTH_CLIENT_SECRET=...
LIVY_OAUTH_AUTH_URL=https://...
LIVY_OAUTH_TOKEN_URL=https://...
LIVY_OAUTH_REDIRECT_URL=http://localhost:3001/auth/livy/callback
```

Optional:

```dotenv
LIVY_OAUTH_INTROSPECTION_URL=https://...
LIVY_OAUTH_SCOPES=openid profile email
LIVY_OAUTH_COOKIE_NAME=livy_resolver_session
LIVY_OAUTH_SESSION_TTL_SECS=28800
LIVY_OAUTH_STATE_TTL_SECS=600
LIVY_OAUTH_INTROSPECTION_CACHE_SECS=60
LIVY_OAUTH_COOKIE_SECURE=false
```

Open `/auth/livy/login` to start login. The resolver redirects to Livy; if the browser already has a Livy session, Livy uses its own cookies and sends the user back without prompting. The resolver then sets its own local MCP session cookie. Non-MCP HTTP routes remain public.

## Verify

```bash
curl -s http://localhost:3001/fetch \
  -H 'content-type: application/json' \
  -d '{"source":"https://example.com","mode":"fast","receipt":true}'
```
