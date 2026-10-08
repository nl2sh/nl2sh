TERMUX_PKG_HOMEPAGE=https://github.com/nl2sh/nl2sh
TERMUX_PKG_DESCRIPTION="Android-first natural-language shell agent with a local safety boundary"
TERMUX_PKG_LICENSE="MIT"
TERMUX_PKG_MAINTAINER="Ernest Su <307141632@qq.com>"
TERMUX_PKG_VERSION="1.1.0"
TERMUX_PKG_SRCURL="https://github.com/nl2sh/nl2sh/archive/refs/tags/v${TERMUX_PKG_VERSION}.tar.gz"
TERMUX_PKG_SHA256=8ce8fb9aff71987aa85dc96047adf83da84ff0ad1b932853c533f4c402da8974
TERMUX_PKG_BUILD_IN_SRC=true
TERMUX_PKG_AUTO_UPDATE=true

termux_step_pre_configure() {
	if [[ -f "${TERMUX_PKG_SRCDIR}/build.rs" ]]; then
		termux_setup_nodejs
	fi
	termux_setup_rust
}

termux_step_make() {
	NL2SH_PACKAGE_MANAGER_BUILD=1 cargo build \
		--locked \
		--no-default-features \
		--jobs "${TERMUX_PKG_MAKE_PROCESSES}" \
		--target "${CARGO_TARGET_NAME}" \
		--release
}

termux_step_make_install() {
	install -Dm755 \
		"target/${CARGO_TARGET_NAME}/release/nl2sh" \
		"${TERMUX_PREFIX}/bin/nl2sh"
	install -Dm644 config.toml.example \
		"${TERMUX_PREFIX}/share/nl2sh/config.toml.example"
	install -Dm644 README.md \
		"${TERMUX_PREFIX}/share/doc/nl2sh/README.md"
}
