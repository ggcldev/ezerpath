# Ezerpath App Workspace

This directory contains the shipped Tauri desktop application:

- `src/` — SolidJS frontend
- `src-tauri/` — Rust backend and Tauri shell

Use the root [`README.md`](../README.md) for architecture and setup details.

## Common commands

```bash
npm install
npm run typecheck
npm test
npm run build
npm run check:rust
npm run test:rust
npm run lint:rust
npm run verify
npx tauri dev
```

## Notes

- `npm run verify` is the local verification path mirrored by CI.
- Ollama is expected to be running locally unless you override the runtime config in-app.
