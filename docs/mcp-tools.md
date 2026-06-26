# MCP Tools

This server exposes one MCP tool: `fetch_source`.

MCP tool annotations are hints for clients. They are not security guarantees, but this project treats them as required documentation for every exposed tool.

## `fetch_source`

Fetch the exact source URL supplied by the user using the fast SmartMode proxy path.

Use this tool when the prompt contains `source: <url>`, "only take this source", "source of truth", or an explicitly required URL. Pass the exact URL. Do not search or substitute another source.

### Tool Definition

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

### Input Schema

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

### Output Schema

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

### Output Text Format

```text
receipt_id: <receipt id>
status: <http status or unknown>
fetch_elapsed_ms: <elapsed milliseconds or unknown>
content_bytes: <byte count or unknown>

---

<extracted page content, or raw JSON payload if no content field exists>
```

### Annotation Rationale

| Annotation | Value | Reason |
|---|---:|---|
| `readOnlyHint` | `true` | The tool fetches source data and does not mutate customer/application state. |
| `destructiveHint` | `false` | It does not delete, overwrite, send messages, charge money, or perform destructive updates. |
| `idempotentHint` | `true` | Repeating the same fetch has no additional external side effect beyond another read/proxy request. |
| `openWorldHint` | `true` | It can access arbitrary caller-provided URLs through an external proxy/fetch service. |

