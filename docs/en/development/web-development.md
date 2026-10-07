# Web development

`web/` uses Preact, TypeScript, Vite, and CSS. `src/web/` serves SSE session state and WebSocket safety terminal; rust-embed packages production assets into one executable.

```bash
cd web
npm ci
npm test
npm run build
```

Manual builds produce `web/dist/`; Cargo builds a copy under target. Do not commit dist/node_modules. Check npm scripts before using a development server, and explicitly configure backend origin/proxy paths rather than assuming that server is the Android backend.

Use style.css semantic variables and UI_DESIGN.md for TUI/Web consistency. Unknown code languages remain escaped plain text. Statistics/charts/reasoning display must not enter model history or security decisions.

Update both languages for Web/config/troubleshooting behavior changes. Test reconnects, approval IDs/review states, credential redaction, and narrow-screen action visibility. Styling tests do not replace execution-safety tests.

## Backend modules

`server.rs` owns listeners, shutdown, routing and embedded assets; `auth.rs` preserves Host/Origin checks. `state.rs` owns session state, checkpoints, redaction and persistence. `agent.rs` runs tasks; `interaction.rs` implements confirmation and streaming adapters; `websocket.rs` publishes SSE state. `routes/` separates sessions, configuration/model discovery, files, tools/apps, memory, runtime information and the WebSocket terminal. `src/web_ui.rs` remains the public compatibility entry point. Frontend sources remain in `web/src/`; Cargo embeds generated assets from OUT_DIR.

Device overview displays the installation owner, correct update entry point, discovered companion versions and recommendations from a verified runtime manifest. Missing signed policies or unavailable components remain unknown; the browser does not claim to know the remote Helper app version.
