#!/usr/bin/env sh
# navigator SessionStart bootstrap: sets download-script.sh's PF_* env
# contract (see download-script.sh's own header) to the navigator CLI's
# name, data dir and pinned version, then execs it unmodified. All
# provisioning logic lives in download-script.sh; this file only supplies
# navigator's own values.
#
# PF_RELEASE_BASE_URL points at this repo's GitHub Releases download root
# (bare "vX.Y.Z" tags per SC-VERSIONING); download-script.sh resolves the
# rest of the URL against release.yml's actual archive + checksums.txt shape.
set -eu

if [ -z "${CLAUDE_PLUGIN_ROOT:-}" ] || [ -z "${CLAUDE_PLUGIN_DATA:-}" ]; then
  echo "navigator bootstrap: CLAUDE_PLUGIN_ROOT/CLAUDE_PLUGIN_DATA not set -- skipping (not running under the plugin runtime?)" >&2
  exit 0
fi

export PF_CLI_NAME="navigator"
export PF_PLUGIN_DATA="${CLAUDE_PLUGIN_DATA}"
export PF_VERSION_FILE="${CLAUDE_PLUGIN_ROOT}/.claude-plugin/plugin.json"
export PF_RELEASE_BASE_URL="${NAVIGATOR_RELEASE_BASE_URL:-https://github.com/johnrichter/navigator/releases/download}"
export PF_ENV_FILE="${CLAUDE_ENV_FILE:-}"

exec "${CLAUDE_PLUGIN_ROOT}/hooks/download-script.sh"
