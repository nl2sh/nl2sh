# Termux

## Install from TUR

```bash
pkg install tur-repo
pkg install nl2sh
nl2sh --version
nl2sh
```

Use `/config` to configure a model on first launch. Termux is a compatibility runtime: detecting `TERMUX_VERSION` or a standard `PREFIX` selects `$PREFIX/bin/sh` and XDG paths, without assuming optional software is installed.

## Independent signed APT repository

This channel publishes `aarch64`, `arm`, and `x86_64`. Add the source:

```bash
mkdir -p "$PREFIX/etc/apt/keyrings"
curl -fsSL https://nl2sh.github.io/nl2sh/nl2sh-repo.gpg   -o "$PREFIX/etc/apt/keyrings/nl2sh.gpg"
echo "deb [signed-by=$PREFIX/etc/apt/keyrings/nl2sh.gpg] https://nl2sh.github.io/nl2sh stable main"   > "$PREFIX/etc/apt/sources.list.d/nl2sh.list"
pkg update
pkg install nl2sh
```

The public key fingerprint is `5230 D3A7 CCBE ED46 16D3 9C51 FC6A D1BC 63F7 D4D8`. Documentation and APT share the site; the repository root URL is preserved.

## Local deb

Download the matching Release package, check `dpkg --print-architecture`, and run `apt install ./nl2sh_VERSION_aarch64.deb` or the `_arm.deb` / `_x86_64.deb` equivalent. Select one version at a time. Desktop Linux binaries and Android debs are not interchangeable; this Release channel does not publish 32-bit x86 (i686) packages.

## Configuration, updates, and removal

Default configuration is `~/.config/nl2sh/config.toml`, with state in `~/.local/state/nl2sh`. `XDG_CONFIG_HOME` and `XDG_STATE_HOME` change the base paths. Explicit configuration paths keep state beside that configuration.

```bash
pkg update
pkg upgrade nl2sh
# Local deb update: apt install the new package again
# Remove the program; user state is retained
apt remove nl2sh
```

Package-manager builds disable in-app self-update. Root is not required for installation, but ordinary Termux UID lacks shell/root UI-automation permissions; see [permission troubleshooting](../troubleshooting/permissions.md). See [development builds](../development/build.md) to create debs.
