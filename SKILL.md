---
name: livy-resolver
description: |
  Use this skill when configuring or working with the Livy Resolver MCP
  service in this repo. It explains how the resolver relates to MCP
  clients, where `LIVY_KEY` must be configured, and how to use the
  exact-source `fetch_source` tool and product HTTP routes.
---

# Livy Resolver

Livy Resolver is this repo's Axum + RMCP service for resolving web
sources through Spider-backed Livy access. It exposes:

- an MCP endpoint at `http://localhost:3001/mcp`
- one MCP tool, `fetch_source`, for exact source-of-truth URL fetching
- HTTP routes for product-style fetch, crawl, map, search, extract,
  screenshot, unblock, and receipt flows

## Required Configuration

The resolver requires:

```dotenv
LIVY_KEY=your-livy-key
```

This repo does not include an MCP config file yet. The code loads `.env`
and then reads `LIVY_KEY` from the resolver process environment at
startup.

Configure the key based on how MCP is wired:

- If the MCP config launches this repo, put `LIVY_KEY` in that server
  entry's `env`.
- If the MCP client connects to an already-running
  `http://localhost:3001/mcp`, configure `LIVY_KEY` where that server
  process is started.
- For local manual runs, use a shell env var or local `.env`.

Example launch config:

```json
{
  "mcpServers": {
    "livy-resolver": {
      "command": "cargo",
      "args": ["run"],
      "cwd": "/path/to/resolver",
      "env": {
        "LIVY_KEY": "your-livy-key"
      }
    }
  }
}
```

## MCP Use

Use `fetch_source` when the user gives an exact URL as the required
source:

```json
{
  "url": "https://example.com/article"
}
```

Do not search first, replace the URL, or infer a different source. Pass
the exact URL and use the returned content plus receipt metadata.

## Local Run

```bash
cargo run
```

Server URL:

```text
http://localhost:3001
```

MCP URL:

```text
http://localhost:3001/mcp
```

## HTTP Routes

Use these for product/API clients:

- `POST /fetch`
- `POST /crawl`
- `POST /map`
- `POST /search`
- `POST /extract`
- `POST /screenshot`
- `POST /fetchfast`
- `POST /fetchunblock`
- `GET /receipt/{id}`

Example:

```bash
curl -s http://localhost:3001/fetch \
  -H 'content-type: application/json' \
  -d '{"source":"https://example.com","mode":"fast","receipt":true}'
```

## Code Map

- `src/main.rs`: mounts HTTP routes and `/mcp`
- `src/mcp.rs`: defines `fetch_source`
- `src/fetch.rs`: reads `LIVY_KEY`, calls Spider, stores receipts
- `src/api.rs`: HTTP route handlers
- `src/types.rs`: request/response types

After changes, run:

```bash
cargo check
```
