# Releases and documentation publishing

## Android releases

`.github/workflows/release.yml` builds AArch64/ARMv7 with NDK r28c/API 26 on `v*` tags, along with JADX helper, self-update binaries/digests, combined ZIP/TAR, Termux debs, and signed APT snapshots before GitHub Release publication. Manual workflow_dispatch creates a draft; drafts do not update the live site.

Local `pack-release.sh` / `pack-release.ps1` produces the same ZIP layout. Archives include bilingual README website links plus bilingual getting-started Markdown and media, without the deleted 使用说明.md. Termux debs contain both language entry points and use package-manager updates.

Before release review versions, both changelogs, config examples, tool references, scripts, TUR version/source SHA-256, and dependency licenses. Run Rust/frontend/script/docs gates and Android ABI validation. A tag does not prove workflow success; inspect final jobs and assets.

## One GitHub Pages site

Documentation and independent Termux APT share `https://nl2sh.github.io/nl2sh/`. Root `dists/`, `pool/`, and `nl2sh-repo.gpg` paths stay fixed. Documentation-only artifacts must not overwrite them.

Releases publish signed `termux-apt-repository.tar.gz`. Docs PRs validate only; master, manual triggers, and successful Release completion deploy: build bilingual docs → fetch latest stable Release APT snapshot → verify signatures/SHA-256 → merge → upload Pages artifact → deploy-pages. For older releases without snapshots, fetch and verify the existing signed site repository. Failures stop deployment rather than publishing without APT. Deployment jobs share a concurrency group.

Set Pages Source to GitHub Actions and permit master in github-pages. The existing `TERMUX_APT_GPG_PRIVATE_KEY` secret is read by Release signing only, never docs builds. Key rotation requires explicitly updating trust configuration instead of disabling verification.

Generated HTML remains a build artifact; do not commit site/ to master. See [official GitHub Pages workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).
