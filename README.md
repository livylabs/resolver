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

Product routes and MCP `tools/call` require a Livy OAuth bearer token by default.
MCP `tools/list` remains public for connector discovery. Local-only
unauthenticated development can set `LIVY_RESOLVER_AUTH_ENABLED=false`.

```dotenv
LIVY_OAUTH_ISSUER=https://auth.livylabs.xyz
LIVY_OAUTH_INTROSPECTION_URL=https://auth.livylabs.xyz/oauth/introspect
LIVY_RESOLVER_OAUTH_AUDIENCE=https://resolver.api.livylabs.xyz
LIVY_RESOLVER_OAUTH_RESOURCE_METADATA_URL=https://resolver.api.livylabs.xyz/.well-known/oauth-protected-resource
```

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
```

Set `LIVY_PROVENANCE_BOOTSTRAP_TEMPLATE=true` only when the API key is
allowed to write provenance templates. Public explorer reads also require
the matching public template to exist in Livy.

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
    "explorer_url": "..."
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
- Protected resource metadata: `/.well-known/oauth-protected-resource` and `/.well-known/oauth-protected-resource/mcp`, including `resource_name` and the Livy OAuth introspection endpoint
- Server: `livygensyn-source-fetcher`
- Tool: `fetch_source` — input `{ "url": "..." }`
- Auth: `tools/call` requires `Authorization: Bearer <livy_oauth_access_token>` with `tool:fetch_source` or `mcp` scope and the resolver audience
- Discovery: unauthenticated `tools/list` is allowed so clients can read the tool descriptor; unauthenticated `fetch_source` calls return `_meta["mcp/www_authenticate"]` with the protected-resource metadata URL, required scope, `error="invalid_request"`, and an error description
- ChatGPT metadata: the `fetch_source` tool descriptor includes top-level `title` and `securitySchemes`, `_meta.securitySchemes`, short invocation status text, and read-only/open-world annotations

Use when the prompt contains `source: <url>`, "only take this source", "source of truth", or an explicitly required URL. Pass the exact URL, don't search or substitute.

## Verify

```bash
curl -s http://localhost:3001/fetch \
  -H 'content-type: application/json' \
  -H "authorization: Bearer $LIVY_OAUTH_ACCESS_TOKEN" \
  -d '{"source":"https://example.com","mode":"fast","receipt":true}'
```
