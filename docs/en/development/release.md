# Releases and documentation publishing

## Android releases

`.github/workflows/release.yml` builds AArch64/ARMv7/x86_64 with NDK r28c/API 26 on `v*` tags, along with self-update binaries/digests, combined ZIP/TAR, Termux debs, and signed APT snapshots before GitHub Release publication. Manual workflow_dispatch creates a draft; drafts do not update the live site.

Local `pack-release.sh` / `pack-release.ps1` produces the same ZIP layout. Archives include bilingual README website links plus bilingual getting-started Markdown and media, without the deleted 使用说明.md. Termux debs contain both language entry points and use package-manager updates.

Before release review versions, both changelogs, config examples, tool references, scripts, TUR version/source SHA-256, and dependency licenses. Run Rust/frontend/script/docs gates and Android ABI validation. A tag does not prove workflow success; inspect final jobs and assets.

## One GitHub Pages site

Documentation and independent Termux APT share `https://nl2sh.github.io/nl2sh/`. Root `dists/`, `pool/`, and `nl2sh-repo.gpg` paths stay fixed. Documentation-only artifacts must not overwrite them.

Releases publish signed `termux-apt-repository.tar.gz`. Docs PRs validate only; master, manual triggers, and successful Release completion deploy: build bilingual docs → fetch latest stable Release APT snapshot → verify signatures/SHA-256 → merge → upload Pages artifact → deploy-pages. For older releases without snapshots, fetch and verify the existing signed site repository. Failures stop deployment rather than publishing without APT. Deployment jobs share a concurrency group.

Set Pages Source to GitHub Actions and permit master in github-pages. The existing `TERMUX_APT_GPG_PRIVATE_KEY` secret is read by Release signing only, never docs builds. Key rotation requires explicitly updating trust configuration instead of disabling verification.

Generated HTML remains a build artifact; do not commit site/ to master. See [official GitHub Pages workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).

### Signed compatibility and assets

`packaging/runtime-components.json` selects independent Bridge/JADX releases and the minimum Helper version. Publish those companion versions first. The release downloads their exact artifacts, validates packaged metadata and the APK signature, then signs `nl2sh-runtime-extensions.json` before the three native builds embed it. A final `nl2sh-runtime.json` adds actual native sizes/digests. Both manifests and each native/Bridge/JADX asset have binary SHA-256 detached GPG signatures. Tag and Cargo version must agree.

Signing reuses `TERMUX_APT_GPG_PRIVATE_KEY` in the github-pages environment, pinned to fingerprint `5230D3A7CCBEED4616D39C51FC6AD1BC63F7D4D8`. Bridge uses its existing APK keystore secrets. If companion repositories are private, `NL2SH_COMPONENTS_TOKEN` must permit reading their releases; otherwise the job uses the repository token. Missing assets, keys, protocol mismatches or invalid APK signatures fail publication. Local production signing may be deferred; unsigned local native builds require explicit offline/custom JADX configuration.

Native and Helper downloads verify the signed manifest before following its asset URLs and verify size, SHA-256 and asset signature before installation. Modern unsigned releases cannot be used for new installation or upgrade. Connecting to a healthy already installed runtime still works without checking releases.

## Runtime refactor acceptance (2026-10-07)

All planned protocol, service, Helper, manifest/signature, descriptor, UI lease/audit, static analysis and Web separation changes are implemented. Local verification:

| Gate | Result |
| --- | --- |
| Rust | Complete workspace tests, all targets, formatter and package-manager feature check passed; existing ignored tests remain ignored |
| Android native | NDK r28c/API 26 release builds for ARM64, ARMv7 and x86_64 passed |
| Android projects | All three Gradle test/lint/debug/release gates passed; production APK signing deferred locally |
| Frontend/docs | 33 frontend tests and production build; 49 bilingual pages per language, strict build and generated references passed |
| Release tooling | 15 packaging/launcher/Pages tests, workflow parsing, and actual three-ELF/APK/JAR manifest generation passed |
| API 26 and 35 emulators | UID 2000 protocol correlation/malformed requests/app UID denial; all eight static tools; cross-process UI exclusion/read independence/recovery; service idempotence/restart/stop/strict collision/fallback port and embedded HTTP passed |
| Helper on both emulators | Healthy reconnect preserves PID; failed update restores binary and owner; matching explicit update records ownership without restart; companion certificate refusal/in-place install/settings preservation; GPG fixture/tamper/policy drift passed (three device tests each) |

The emulator checks use local unsigned native builds, debug APKs and public test-signature fixtures. ARM builds were validated as ELF artifacts, not executed on ARM hardware in this acceptance run. Production GPG/APK signatures and published release bytes must be verified after GitHub Actions runs; no tags or remote releases were created by this implementation.
