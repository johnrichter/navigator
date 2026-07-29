#!/usr/bin/env sh
# navigator PreToolUse wrapper: points plugin-foundation's forced-use-hook.sh at
# this plugin's own routing-rules.json and hookeval log, then execs it unmodified
# (stdin -- the PreToolUse payload -- passes through exec untouched). All
# deny-and-redirect and adoption-logging logic lives in forced-use-hook.sh; this file
# only supplies navigator's own paths.
set -eu

export PF_ROUTING_RULES="${CLAUDE_PLUGIN_ROOT}/routing-rules.json"
if [ -n "${CLAUDE_PLUGIN_DATA:-}" ]; then
  export PF_HOOKEVAL_LOG="${CLAUDE_PLUGIN_DATA}/hookeval.jsonl"
fi

exec "${CLAUDE_PLUGIN_ROOT}/hooks/forced-use-hook.sh"
