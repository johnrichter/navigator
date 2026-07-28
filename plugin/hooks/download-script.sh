#!/usr/bin/env sh
# download-script.sh -- generic per-OS/arch binary provisioner shared by every
# govern-now CLI's plugin. A plugin never copies this file; its SessionStart
# hook execs it with the env below set to that plugin's own CLI name, data
# dir, and release host.
#
# Ladder: (1) a cached binary for the pinned version that still matches its
# own recorded sha256 -- no network; (2) download the per-arch archive, verify
# it against the release's checksums, extract the binary, cache it and its
# own digest. Anything short of a verified binary is a soft failure (no
# crash): the caller (typically forced-use-hook.sh, indirectly, via
# CLI-availability probing) treats an unresolved binary the same as a CLI
# that was never installed and fails open to the raw OS tool.
#
# Inputs (env):
#   PF_CLI_NAME          required -- the governed CLI's name. Used to build
#                        the cached binary's filename and the archive's
#                        filename, and (unless PF_BIN_ENV overrides it) the
#                        exported env var name: PF_CLI_NAME with '-' -> '_',
#                        uppercased, suffixed "_BIN".
#   PF_PLUGIN_DATA       required -- persistent per-plugin data directory
#                        (Claude Code's CLAUDE_PLUGIN_DATA). The version-
#                        keyed binary cache lives at "$PF_PLUGIN_DATA/bin".
#   PF_RELEASE_BASE_URL  required -- release host root. A file:// URL for
#                        tests and air-gapped mirrors, https:// otherwise.
#   PF_VERSION           the pinned version string. Takes precedence over
#                        PF_VERSION_FILE when both are set.
#   PF_VERSION_FILE      path to a plugin.json-shaped file; the pinned
#                        version is read from its top-level "version" field.
#                        Required when PF_VERSION is unset.
#   PF_BIN_ENV           env var name the verified binary path is exported
#                        under. Default: derived from PF_CLI_NAME (above).
#   PF_ENV_FILE          file to append `export $PF_BIN_ENV=...` to
#                        (Claude Code's CLAUDE_ENV_FILE). Unset: skip export,
#                        the resolved path still prints to stdout.
#   PF_ARCH_OVERRIDE     test-only: skip uname resolution, use this "os/arch"
#                        pair verbatim (e.g. "linux/amd64").
#
# Release layout expected under PF_RELEASE_BASE_URL:
#   v<version>/<name>_<version>_<os>_<arch>.tar.gz   the archive, containing
#                                                     exactly one file: the
#                                                     binary.
#   v<version>/checksums.txt                         one shared digest list
#                                                     for every archive in the
#                                                     tag, "<sha256>  <archive
#                                                     filename>" per line.
#   v<version>/<archive filename>.sha256              per-artifact fallback,
#                                                      only when a repo still
#                                                      publishes one; used
#                                                      when checksums.txt is
#                                                      unreachable or lacks
#                                                      this archive.
#
# Outputs:
#   "$PF_PLUGIN_DATA/bin/<name>-<version>"          the verified binary
#   "$PF_PLUGIN_DATA/bin/<name>-<version>.sha256"   its own digest, recorded
#                                                    for the cache fast path
#   stdout: the verified binary's absolute path (success only)
#   $PF_ENV_FILE: `export $PF_BIN_ENV=<path>` (success only, when set)
#
# Exit codes: 0 verified (path on stdout); 1 no verified binary produced
# (unreachable host, sha256 mismatch, unsupported arch, unresolved version --
# a soft failure, never a crash); 2 misconfigured (a required env var is
# unset) -- distinct from 1 because it is a plugin wiring bug, not a runtime
# provisioning outcome.
set -eu

warn() {
  echo "download-script: $*" >&2
}

require_env() {
  eval "val=\${$1:-}"
  if [ -z "${val}" ]; then
    warn "required env var $1 is not set"
    exit 2
  fi
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    return 1
  fi
}

# sha256_sidecar_value -- prints the hex digest a sidecar file names, whether
# it holds a bare digest or a `sha256sum`-style "<hash>  <filename>" line.
sha256_sidecar_value() {
  awk '{print $1; exit}' "$1" 2>/dev/null
}

# checksum_for_artifact CHECKSUMS_FILE ARCHIVE_NAME -- prints the digest
# CHECKSUMS_FILE records for ARCHIVE_NAME (one "<sha256>  <filename>" line
# per archive the tag ships), or nothing if that filename isn't listed.
checksum_for_artifact() {
  awk -v f="$2" '$2 == f { print $1; exit }' "$1" 2>/dev/null
}

# resolve_os_arch -- prints "<os>/<arch>" using Go's GOOS/GOARCH vocabulary
# (this ecosystem's build matrices, and so its release archive names, are
# keyed on it), from PF_ARCH_OVERRIDE verbatim or from uname otherwise.
resolve_os_arch() {
  if [ -n "${PF_ARCH_OVERRIDE:-}" ]; then
    echo "${PF_ARCH_OVERRIDE}"
    return
  fi
  kernel="$(uname -s)"
  machine="$(uname -m)"
  case "${kernel}" in
  Linux) os="linux" ;;
  Darwin) os="darwin" ;;
  *)
    warn "unsupported kernel '${kernel}'"
    return 1
    ;;
  esac
  case "${machine}" in
  x86_64 | amd64) arch="amd64" ;;
  arm64 | aarch64) arch="arm64" ;;
  *)
    warn "unsupported machine type '${machine}'"
    return 1
    ;;
  esac
  echo "${os}/${arch}"
}

# read_version -- prints the pinned version: PF_VERSION verbatim, or the
# "version" field of PF_VERSION_FILE (jq when available, else a grep/sed
# fallback matching plugin.json's fixed one-field-per-line shape).
read_version() {
  if [ -n "${PF_VERSION:-}" ]; then
    printf '%s' "${PF_VERSION}"
    return 0
  fi
  if [ -z "${PF_VERSION_FILE:-}" ]; then
    warn "neither PF_VERSION nor PF_VERSION_FILE is set"
    return 1
  fi
  if [ ! -f "${PF_VERSION_FILE}" ]; then
    warn "PF_VERSION_FILE not found: ${PF_VERSION_FILE}"
    return 1
  fi
  if command -v jq >/dev/null 2>&1; then
    jq -r '.version' "${PF_VERSION_FILE}"
    return
  fi
  grep -m1 '"version"' "${PF_VERSION_FILE}" | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]*)".*/\1/'
}

# fetch SRC DEST -- src may be file:// (tests, air-gapped mirrors) or
# http(s)://, fetched with whichever of curl/wget is on PATH.
fetch() {
  case "$1" in
  file://*)
    cp "${1#file://}" "$2"
    ;;
  *)
    if command -v curl >/dev/null 2>&1; then
      curl -fsSL -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
      wget -q -O "$2" "$1"
    else
      warn "neither curl nor wget is available -- cannot fetch $1"
      return 1
    fi
    ;;
  esac
}

# default_bin_env NAME -- NAME with '-' -> '_', uppercased, suffixed "_BIN".
default_bin_env() {
  printf '%s' "$1" | tr '[:lower:]-' '[:upper:]_'
  printf '_BIN'
}

require_env PF_CLI_NAME
require_env PF_PLUGIN_DATA
require_env PF_RELEASE_BASE_URL

bin_env="${PF_BIN_ENV:-$(default_bin_env "${PF_CLI_NAME}")}"

version="$(read_version || true)"
if [ -z "${version}" ]; then
  warn "could not resolve a pinned version"
  exit 1
fi

bin_dir="${PF_PLUGIN_DATA}/bin"
bin_path="${bin_dir}/${PF_CLI_NAME}-${version}"
bin_sidecar="${bin_path}.sha256"
verified=0

# Idempotent cache fast path: bytes already on disk that still match their
# own recorded digest need no network round trip at all.
if [ -f "${bin_path}" ] && [ -f "${bin_sidecar}" ]; then
  recorded="$(sha256_sidecar_value "${bin_sidecar}" || true)"
  actual="$(sha256_of "${bin_path}" 2>/dev/null || true)"
  if [ -n "${recorded}" ] && [ "${recorded}" = "${actual}" ]; then
    verified=1
  else
    warn "cached ${bin_path} failed local re-verification -- re-downloading"
  fi
fi

if [ "${verified}" -eq 0 ]; then
  os_arch="$(resolve_os_arch || true)"
  if [ -z "${os_arch}" ]; then
    exit 1
  fi
  os="${os_arch%%/*}"
  arch="${os_arch#*/}"

  tag_dir="v${version}"
  archive_name="${PF_CLI_NAME}_${version}_${os}_${arch}.tar.gz"
  release_dir_url="${PF_RELEASE_BASE_URL}/${tag_dir}"
  archive_url="${release_dir_url}/${archive_name}"

  mkdir -p "${bin_dir}"
  checksums_tmp="$(mktemp "${bin_dir}/.checksums.XXXXXX")"
  sidecar_tmp="$(mktemp "${bin_dir}/.sha256.XXXXXX")"
  archive_tmp="$(mktemp "${bin_dir}/.download.XXXXXX")"

  expected=""
  if fetch "${release_dir_url}/checksums.txt" "${checksums_tmp}" 2>/dev/null; then
    expected="$(checksum_for_artifact "${checksums_tmp}" "${archive_name}" || true)"
  fi
  if [ -z "${expected}" ] && fetch "${archive_url}.sha256" "${sidecar_tmp}" 2>/dev/null; then
    expected="$(sha256_sidecar_value "${sidecar_tmp}" || true)"
  fi

  if [ -n "${expected}" ] && fetch "${archive_url}" "${archive_tmp}"; then
    actual="$(sha256_of "${archive_tmp}" 2>/dev/null || true)"
    if [ -n "${actual}" ] && [ "${expected}" = "${actual}" ]; then
      extract_dir="$(mktemp -d "${bin_dir}/.extract.XXXXXX")"
      if tar -xzf "${archive_tmp}" -C "${extract_dir}" 2>/dev/null; then
        entry="$(find "${extract_dir}" -type f | head -n1)"
        if [ -n "${entry}" ]; then
          chmod +x "${entry}"
          mv "${entry}" "${bin_path}"
          sha256_of "${bin_path}" >"${bin_sidecar}"
          verified=1
        else
          warn "archive ${archive_name} contained no extractable file"
        fi
      else
        warn "failed to extract archive ${archive_name}"
      fi
      rm -rf "${extract_dir}"
    else
      warn "sha256 mismatch for ${archive_name} -- expected ${expected:-<none>}, got ${actual:-<none>}"
    fi
  else
    warn "failed to fetch ${archive_url} (or resolve its expected digest from checksums.txt/sidecar)"
  fi
  rm -f "${checksums_tmp}" "${sidecar_tmp}" "${archive_tmp}"
fi

if [ "${verified}" -ne 1 ]; then
  warn "no verified binary for ${PF_CLI_NAME} ${version}"
  exit 1
fi

if [ -n "${PF_ENV_FILE:-}" ]; then
  echo "export ${bin_env}=\"${bin_path}\"" >>"${PF_ENV_FILE}"
fi
echo "${bin_path}"
