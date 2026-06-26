# Livy Web Resolver

Default mode is SmartMode. Override via `mode` when you need specific cost/speed/browser/unblock behavior.

## Status

- [x] Browserful query and browserless URL resolver
- [x] Sanitized markdown and request params
- [x] Proxy enabling for complex requests
- [ ] Residential proxy rotations
- [ ] Prompt-based interaction with results
- [ ] MCP auth
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
- Tool: `fetch_source`

Use when the prompt contains `source: <url>`, "only take this source", "source of truth", or an explicitly required URL. Pass the exact URL, don't search or substitute.

### Tool Contract

`fetch_source` fetches one caller-provided URL through the fast SmartMode proxy path and returns receipt metadata plus extracted source content. It is the only MCP tool exposed by this server.

The standalone tool contract is also available at [`docs/mcp-tools.md`](docs/mcp-tools.md).

Tool definition:

```json
{
  "name": "fetch_source",
  "description": "Fetch the exact source URL supplied by the user using the fast SmartMode proxy path. Use this whenever the prompt contains a source URL, `source: <url>`, `only take this source`, or says the URL is the source of truth. Do not perform web search or substitute another article.",
  "annotations": {
    "title": "Fetch exact source URL",
    "readOnlyHint": true,
    "destructiveHint": false,
    "idempotentHint": true,
    "openWorldHint": true
  }
}
```

Input schema:

```json
{
  "type": "object",
  "properties": {
    "url": {
      "type": "string",
      "description": "The exact source URL from the user prompt. Do not replace it with a search query or another URL."
    }
  },
  "required": ["url"],
  "additionalProperties": false
}
```

Call example:

```json
{
  "url": "https://example.com/article"
}
```

Output schema:

```json
{
  "type": "object",
  "properties": {
    "content": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "type": { "const": "text" },
          "text": {
            "type": "string",
            "description": "Plain text containing receipt_id, status, fetch_elapsed_ms, content_bytes, then the fetched source content after a --- separator."
          }
        },
        "required": ["type", "text"]
      }
    },
    "isError": { "type": "boolean" }
  },
  "required": ["content"]
}
```

Successful text content format:

```text
receipt_id: <receipt id>
status: <http status or unknown>
fetch_elapsed_ms: <elapsed milliseconds or unknown>
content_bytes: <byte count or unknown>

---

<extracted page content, or raw JSON payload if no content field exists>
```

Annotation rationale:

| Annotation | Value | Reason |
|---|---:|---|
| `readOnlyHint` | `true` | The tool fetches source data and does not mutate customer/application state. |
| `destructiveHint` | `false` | It does not delete, overwrite, send messages, charge money, or perform destructive updates. |
| `idempotentHint` | `true` | Repeating the same fetch has no additional external side effect beyond another read/proxy request. |
| `openWorldHint` | `true` | It can access arbitrary caller-provided URLs through an external proxy/fetch service. |

## Verify

```bash
curl -s http://localhost:3001/fetch \
  -H 'content-type: application/json' \
  -d '{"source":"https://example.com","mode":"fast","receipt":true}'
```
