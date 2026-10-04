#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <directory-containing-debs> <repository-output>" >&2
  exit 2
fi

DEB_DIR="$1"
REPO_DIR="$2"
CODENAME="stable"
COMPONENT="main"

command -v apt-ftparchive >/dev/null || {
  echo "apt-ftparchive is required (Debian/Ubuntu package: apt-utils)" >&2
  exit 1
}

if [[ -e "${REPO_DIR}" ]] && find "${REPO_DIR}" -mindepth 1 -print -quit | grep -q .; then
  echo "repository output must be absent or empty: ${REPO_DIR}" >&2
  exit 1
fi
mkdir -p "${REPO_DIR}/pool/main/n/nl2sh"
find "${DEB_DIR}" -maxdepth 1 -type f -name 'nl2sh_*.deb' -exec cp {} "${REPO_DIR}/pool/main/n/nl2sh/" \;

for arch in aarch64 arm x86_64; do
  packages_dir="${REPO_DIR}/dists/${CODENAME}/${COMPONENT}/binary-${arch}"
  mkdir -p "${packages_dir}"
  (cd "${REPO_DIR}" && apt-ftparchive -a "${arch}" packages pool) > "${packages_dir}/Packages"
  gzip -9n -c "${packages_dir}/Packages" > "${packages_dir}/Packages.gz"
done

cat > "${REPO_DIR}/apt-release.conf" <<EOF
APT::FTPArchive::Release::Origin "nl2sh";
APT::FTPArchive::Release::Label "nl2sh Termux repository";
APT::FTPArchive::Release::Suite "${CODENAME}";
APT::FTPArchive::Release::Codename "${CODENAME}";
APT::FTPArchive::Release::Architectures "aarch64 arm x86_64";
APT::FTPArchive::Release::Components "${COMPONENT}";
APT::FTPArchive::Release::Description "Signed nl2sh packages for Termux";
EOF

release_dir="${REPO_DIR}/dists/${CODENAME}"
apt-ftparchive -c "${REPO_DIR}/apt-release.conf" release "${release_dir}" > "${release_dir}/Release"
rm "${REPO_DIR}/apt-release.conf"

if [[ -n "${NL2SH_APT_GPG_KEY_ID:-}" ]]; then
  gpg --batch --yes --local-user "${NL2SH_APT_GPG_KEY_ID}" \
    --clearsign --output "${release_dir}/InRelease" "${release_dir}/Release"
  gpg --batch --yes --local-user "${NL2SH_APT_GPG_KEY_ID}" \
    --armor --detach-sign --output "${release_dir}/Release.gpg" "${release_dir}/Release"
  gpg --batch --yes --local-user "${NL2SH_APT_GPG_KEY_ID}" \
    --export "${NL2SH_APT_GPG_KEY_ID}" > "${REPO_DIR}/nl2sh-repo.gpg"
else
  echo "repository indexes generated without signatures; set NL2SH_APT_GPG_KEY_ID for publication" >&2
fi
