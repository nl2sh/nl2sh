# Contributing and bilingual documentation

Read root AGENTS.md, ARCHITECTURE.md, PROJECT_PLAN.md, and PROJECT_STATUS.md before edits. `docs/zh/` and `docs/en/` are the user-documentation source of truth. Root/module READMEs retain introductions, development entry points, and links.

## Update code and docs together

| User-visible change | Pages to check in both languages |
| --- | --- |
| Install, launch, update | getting-started/installation, android-adb, termux |
| Providers/proxies | configure-provider, advanced/custom-provider, proxy |
| Config, CLI, environment | reference/configuration, cli, environment-variables |
| Tools/arguments/enabling | Domain tools pages, reference/tool-catalog |
| Slash commands | reference/slash-commands |
| Approval/Root | guide/security-confirmation, root, reference/security-model |
| Web/sessions/references | Corresponding guide and troubleshooting |
| Releases | development/release, changelog |

New pages use matching relative paths with substantive translations. Chinese fallback is not a completed English page. Synchronize navigation, captions, examples, risks, and defaults. Declare documentation updates in PRs, or explain why none are needed.

## Local documentation commands

```bash
python3 -m venv .venv-docs
.venv-docs/bin/python -m pip install -r requirements-docs.txt
.venv-docs/bin/python scripts/check-docs.py
.venv-docs/bin/mkdocs build --strict
.venv-docs/bin/mkdocs serve
```

Windows uses `.venv-docs\Scripts\python.exe` / `mkdocs.exe`. Chinese is at the site root; English at `/en/`. Language switching retains the current page and edit links point to the actual translated source.

Update code-derived references:

```bash
cargo run --quiet --example docs_export > /tmp/nl2sh-reference.json
python3 scripts/update-docs-reference.py /tmp/nl2sh-reference.json
python3 scripts/update-docs-reference.py /tmp/nl2sh-reference.json --check
```

Add Chinese config/tool descriptions to `docs/_data/reference-translations.json`. Generated sections preserve protocol text; authored paragraphs explain behavior/risk/platforms. Every PR checks parity, links, reference freshness, and strict builds; master deploys automatically.

## Code checks

Run cargo fmt/check/test and applicable Android/security/PTY restoration checks. Use stable Rust, avoid production unwrap/expect/panic, isolate blocking I/O, and manage resources with RAII. Project records contain reproducible commands/platforms/results, not usernames, local absolute paths, or session authorization.
