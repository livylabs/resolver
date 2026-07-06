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

Product routes and protected MCP requests require a Livy OAuth bearer token by default.
MCP `initialize`, `notifications/initialized`, and `tools/list` remain public for connector discovery.
Unauthenticated MCP probes and `tools/call` return HTTP `401` with `WWW-Authenticate`
so Claude can discover the OAuth protected resource. Local-only
unauthenticated development can set `LIVY_RESOLVER_AUTH_ENABLED=false`.

```dotenv
LIVY_OAUTH_ISSUER=https://auth.livylabs.xyz
LIVY_OAUTH_INTROSPECTION_URL=https://auth.livylabs.xyz/oauth/introspect
LIVY_RESOLVER_OAUTH_AUDIENCE=https://resolver.api.livylabs.xyz/mcp
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
LIVY_INTEGRATION_ID=delphi
ITA_API_KEY=...
```

For OAuth-protected MCP `fetch_source` calls, provenance writes are posted to
`/api/v1/resolver/source-fetch-attestations` with the user's Livy OAuth bearer
token. The backend derives tenant/project from the token and rejects
project-less resolver tokens. Do not grant ChatGPT clients generic
`provenance:attestation:write`.

Optional settings:

```dotenv
LIVY_API_KEY=livy_...
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

`LIVY_API_KEY` is only used for legacy/local service-key provenance writes.
Production service-key writes are disabled unless
`LIVY_PROVENANCE_ALLOW_SERVICE_API_KEY=true` is set.

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
- Protected resource metadata: `/.well-known/oauth-protected-resource` and `/.well-known/oauth-protected-resource/mcp`, including `resource_name` and the Livy OAuth introspection endpoint
- Server: `livygensyn-source-fetcher`
- Tool: `fetch_source` — input `{ "url": "..." }`
- Auth: protected MCP requests require `Authorization: Bearer <livy_oauth_access_token>` with `tool:fetch_source` or `mcp` scope and the resolver MCP endpoint audience
- Discovery: unauthenticated `initialize`, `notifications/initialized`, and `tools/list` are allowed so clients can read the tool descriptor; unauthenticated MCP probes and `fetch_source` calls return HTTP `401` with `WWW-Authenticate` pointing at the protected-resource metadata URL. The tool implementation keeps `_meta["mcp/www_authenticate"]` compatibility for contexts that reach tool dispatch directly.
- ChatGPT metadata: the `fetch_source` tool descriptor includes top-level `title` and `securitySchemes`, `_meta.securitySchemes`, short invocation status text, and read-only/open-world annotations

Use when the prompt contains `source: <url>`, "only take this source", "source of truth", or an explicitly required URL. Pass the exact URL, don't search or substitute.

## Verify

```bash
curl -s http://localhost:3001/fetch \
  -H 'content-type: application/json' \
  -H "authorization: Bearer $LIVY_OAUTH_ACCESS_TOKEN" \
  -d '{"source":"https://example.com","mode":"fast","receipt":true}'
```
