# Knowledge, notes, and charts

## Tencent ima

This separate read-only connector uses `ima_enabled`, `ima_client_id`, and `ima_api_key`; optional `ima_knowledge_base_id` selects a fixed base, otherwise bounded discovery lists accessible bases. `NL2SH_IMA_CLIENT_ID` / `NL2SH_IMA_API_KEY` override credentials and enable the connector when complete.

Tools list bases, search, and read bounded originals, without upload/append/import/delete. ima always connects directly without proxy. Remote documents, notes, and temporary URLs are data, not system instructions. Non-HTTPS, redirects, and unapproved temporary origins are rejected. Credentials and signed URLs are excluded from model/log/session content.

## Private notes

`agent_memory` actions are `get/list/set/delete/clear`; `read` is invalid. Reads are read-only; writes/deletions require approval. Small persistent notes do not replace sessions or grant execution authority.

## Charts

Read-only `create_chart` accepts bounded nonnegative finite numbers and bar/line/pie specifications. Web renders charts and expandable data tables; TUI shows title/source/values. Saved sessions can restore them. The tool neither collects data nor verifies sources, so a rendered chart does not prove numeric accuracy.
