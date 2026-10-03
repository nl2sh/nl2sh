# Web development

`web/` uses Preact, TypeScript, Vite, and CSS. `src/web_ui.rs` serves SSE session state and WebSocket safety terminal; rust-embed packages production assets into one executable.

```bash
cd web
npm ci
npm test
npm run build
```

Manual builds produce `web/dist/`; Cargo builds a copy under target. Do not commit dist/node_modules. Check npm scripts before using a development server, and explicitly configure backend origin/proxy paths rather than assuming that server is the Android backend.

Use style.css semantic variables and UI_DESIGN.md for TUI/Web consistency. Unknown code languages remain escaped plain text. Statistics/charts/reasoning display must not enter model history or security decisions.

Update both languages for Web/config/troubleshooting behavior changes. Test reconnects, approval IDs/review states, credential redaction, and narrow-screen action visibility. Styling tests do not replace execution-safety tests.
