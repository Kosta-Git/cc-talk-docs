# cc-talk-docs

Semantic search over the [ccTalk](https://en.wikipedia.org/wiki/CcTalk) serial protocol specification (v4.7, parts 1–4), exposed as an MCP (stdio or http) server or a plain HTTP API. Ask questions like *"how do I read coin selector events?"* and get the relevant spec sections back with document, section, and page references.

## How it works

The PDFs in `docs/` are split into section-aware chunks, embedded with [BGE-small-en-v1.5](https://huggingface.co/Xenova/bge-small-en-v1.5) via [fastembed](https://crates.io/crates/fastembed), and stored in SQLite with [sqlite-vec](https://github.com/asg017/sqlite-vec) for vector search (`database.db`, checked in — no seeding needed to just run it).

Cargo workspace with three crates:

| Crate | Purpose |
|---|---|
| `common` | PDF extraction (pdfium), chunking, embeddings, database access |
| `database-seeder` | Builds `database.db` from the PDFs |
| `api` | Serves search — MCP (stdio) or HTTP, selected by CLI arg |

## Usage

Requires Rust 1.95+. Pdfium binaries and the tokenizer are bundled in `shared/`.

```sh
just start-mcp        # MCP server on stdio
just start-http       # HTTP server
just build-database   # re-seed database.db from ./docs
```

`DOCS_ROOT` (default `./docs`) sets where raw page reads come from.

### MCP

Two tools: `search_docs(query, limit?)` and `get_doc(document, page_start, count?)`. Served two ways:

- **stdio** — `just start-mcp`
- **streamable HTTP** — `just start-http` also mounts MCP at `http://127.0.0.1:8080/mcp`

This repo's `.mcp.json` points Claude Code at the HTTP endpoint (start the server first):

```json
{
  "mcpServers": {
    "cc-talk-docs": {
      "type": "http",
      "url": "http://127.0.0.1:8080/mcp"
    }
  }
}
```

### HTTP

- `GET /cc-talk-docs?query=...&limit=3` — semantic search, returns scored chunks with metadata
- `GET /cc-talk-docs/pages?document=cctalk-part-2-v4-7&page_start=14&count=2` — raw page text
- `GET /health`

Ready-made requests live in `dev/`: a [Bruno](https://www.usebruno.com/) collection (`bruno-cc-talk-docs.yml`) and plain `.request` files with raw HTTP requests for each endpoint.
