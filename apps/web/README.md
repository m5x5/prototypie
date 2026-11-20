# Prototypie (web)

The Next.js implementation of [Prototypie](../../README.md). See the repository README for what it does and the native
app in [`../native`](../native).

```sh
pnpm install
cp .env.example .env     # optional: ANTHROPIC_API_KEY
pnpm dev                 # http://localhost:3000
```

Data is stored in `data/` (git-ignored). Optional settings, all environment variables:

| Variable | Effect |
|---|---|
| `ANTHROPIC_API_KEY` | Generate through the Anthropic API |
| `PROTOTYPIE_PROJECTS_TTL` | A Turtle file of projects for "+ → Space" |
| `PROTOTYPIE_DEV_ORIGINS` | Comma-separated hostnames allowed to open the dev server (other devices) |
| `POD_BASE_<ID>` | Override a Solid pod's base URL |

This Next.js version has breaking changes compared to older ones: read `node_modules/next/dist/docs/` before changing
framework code (see `AGENTS.md`).
