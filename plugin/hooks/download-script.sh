#!/usr/bin/env sh
# download-script.sh -- per-OS/arch binary provisioner for this plugin's
# SessionStart hook. Adapted from plugin-foundation's shared download-script.sh
# contract (same PF_* env inputs/outputs, same cache-then-fetch-then-verify
# ladder) to this repo's actual release layout: a bare "vX.Y.Z" release tag
# (SC-VERSIONING; navigator's one Cargo package lives at the repo root, so it
# has no path-prefix segment to fold into the tag) holding per-OS/arch tar.gz
# archives plus one shared checksums.txt, rather than the foundation's
# per-CLI-prefixed tag directory and per-artifact .sha256 sidecar. Every
# other plugin in this ecosystem still wires the foundation's file verbatim;
# this release shape (shared with git-tools) is the one that needs the
# adapted copy.
#
# Ladder: (1) a cached binary for the pinned version that still matches its
# own recorded sha256 -- no network; (2) download the per-arch archive + the
# release's checksums.txt, verify the archive's digest, extract, cache.
# Anything short of a verified binary is a soft failure (no crash): the
# caller (typically forced-use-hook.sh, indirectly, via CLI-availability
# probing) treats an unresolved binary the same as a CLI that was never
# installed and fails open to the raw OS tool.
#
# Inputs (env):
#   PF_CLI_NAME          required -- the governed CLI's name. Matches both
#                        the archive's embedded binary name and the archive
#                        filename's leading segment (release.yml's build,
#                        "navigator"), and (unless PF_BIN_ENV overrides it)
#                        the exported env var name:
#                        PF_CLI_NAME with '-' -> '_', uppercased, suffixed
#                        "_BIN".
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
# Release layout expected under PF_RELEASE_BASE_URL (this repo's
# .github/workflows/release.yml):
#   v<version>/<name>_<version>_<os>_<arch>.tar.gz   archive holding one
#                                                     file, "<name>", the
#                                                     binary itself
#   v<version>/checksums.txt                         `sha256sum`-style
#                                                     "<hash>  <filename>"
#                                                     lines, one per archive
#                                                     under this tag
#
# Outputs:
#   "$PF_PLUGIN_DATA/bin/<name>-<version>"          the verified binary
#   "$PF_PLUGIN_DATA/bin/<name>-<version>.sha256"   its own digest, recorded
#                                                    post-extraction for the
#                                                    idempotent cache fast path
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

# checksums_lookup FILE NAME -- prints the hex digest checksums.txt (FILE)
# records for filename NAME, or nothing (empty, no error) when absent.
checksums_lookup() {
  awk -v name="$2" '$2 == name { print $1; exit }' "$1" 2>/dev/null
}

# resolve_target -- prints "<os>/<arch>" matching release.yml's Go
# GOOS/GOARCH build matrix (linux/darwin, amd64/arm64).
resolve_target() {
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
sha_sidecar="${bin_path}.sha256"
verified=0

# Idempotent cache fast path: bytes already on disk that still match their
# own recorded digest need no network round trip at all.
if [ -f "${bin_path}" ] && [ -f "${sha_sidecar}" ]; then
  recorded="$(sha256_sidecar_value "${sha_sidecar}" || true)"
  actual="$(sha256_of "${bin_path}" 2>/dev/null || true)"
  if [ -n "${recorded}" ] && [ "${recorded}" = "${actual}" ]; then
    verified=1
  else
    warn "cached ${bin_path} failed local re-verification -- re-downloading"
  fi
fi

if [ "${verified}" -eq 0 ]; then
  target="$(resolve_target || true)"
  if [ -z "${target}" ]; then
    exit 1
  fi
  os="${target%/*}"
  arch="${target#*/}"

  archive_name="${PF_CLI_NAME}_${version}_${os}_${arch}.tar.gz"
  tag_dir="${PF_RELEASE_BASE_URL}/v${version}"
  archive_url="${tag_dir}/${archive_name}"
  checksums_url="${tag_dir}/checksums.txt"

  mkdir -p "${bin_dir}"
  checksums_tmp="$(mktemp "${bin_dir}/.checksums.XXXXXX")"
  archive_tmp="$(mktemp "${bin_dir}/.download.XXXXXX")"
  extract_dir="$(mktemp -d "${bin_dir}/.extract.XXXXXX")"

  if fetch "${checksums_url}" "${checksums_tmp}" && fetch "${archive_url}" "${archive_tmp}"; then
    expected="$(checksums_lookup "${checksums_tmp}" "${archive_name}" || true)"
    actual="$(sha256_of "${archive_tmp}" 2>/dev/null || true)"
    if [ -n "${expected}" ] && [ "${expected}" = "${actual}" ]; then
      if tar -xzf "${archive_tmp}" -C "${extract_dir}" "${PF_CLI_NAME}" 2>/dev/null && [ -f "${extract_dir}/${PF_CLI_NAME}" ]; then
        chmod +x "${extract_dir}/${PF_CLI_NAME}"
        mv "${extract_dir}/${PF_CLI_NAME}" "${bin_path}"
        sha256_of "${bin_path}" >"${sha_sidecar}"
        verified=1
      else
        warn "archive ${archive_name} did not contain a '${PF_CLI_NAME}' binary"
      fi
    else
      warn "sha256 mismatch for ${archive_name} -- expected ${expected:-<none>}, got ${actual:-<none>}"
    fi
  else
    warn "failed to fetch ${archive_url} (or checksums.txt)"
  fi
  rm -rf "${checksums_tmp}" "${archive_tmp}" "${extract_dir}"
fi

if [ "${verified}" -ne 1 ]; then
  warn "no verified binary for ${PF_CLI_NAME} ${version}"
  exit 1
fi

if [ -n "${PF_ENV_FILE:-}" ]; then
  echo "export ${bin_env}=\"${bin_path}\"" >>"${PF_ENV_FILE}"
fi
echo "${bin_path}"
