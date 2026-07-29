#!/usr/bin/env sh
# forced-use-hook.sh -- generic PreToolUse deny-and-redirect hook shared by
# every govern-now CLI's plugin. A plugin never copies this file; it wires it
# into hooks.json against its own routing-rules.json (the same file
# BuildRegistry reads for adoption measurement, so the live decision and the
# measured decision can never drift apart).
#
# Reads the PreToolUse payload on stdin and decides, per governed operation
# whose raw route names this invocation's tool:
#   raw usage, CLI available    -> deny, redirecting to the CLI (never
#                                   claiming the raw tool doesn't exist)
#   raw usage, CLI unavailable  -> fail open: allow, no decision emitted
#   not a raw-route match       -> not applicable: no decision, just logged
#
# The first operation (routing-rules.json order) whose raw route matches
# this invocation produces the decision and stops evaluation; operations
# considered before it that didn't match are logged as not_applicable, and
# an operation whose raw.tool_name differs from this invocation's tool is
# skipped (not evaluated, not logged) as irrelevant to this call.
#
# Inputs (env):
#   PF_ROUTING_RULES  required -- path to the plugin's routing-rules.json
#                     (see routing-rules.schema.json).
#   PF_HOOKEVAL_LOG   optional -- path to append one adoption.HookEvalRecord
#                     JSON line per operation this invocation was evaluated
#                     against. Unset: the routing decision still applies,
#                     it just isn't logged for adoption measurement.
#
# Inputs (stdin): the PreToolUse hook JSON payload. Fields read: `tool_name`,
# `session_id`, `tool_input.command` (Bash invocations only).
#
# Outputs (stdout): a PreToolUse hookSpecificOutput JSON with
# permissionDecision "deny" on redirect, or nothing at all (silent allow)
# otherwise.
#
# Fail-open invariants: jq missing, payload unparseable, PF_ROUTING_RULES
# missing/invalid, or no CLI available for a matched raw operation --  every
# one of these degrades to a silent allow, never a deny. This hook never
# denies a raw tool call by claiming the tool doesn't exist; every logged
# record carries denies_tool_exists=false by construction, satisfying the
# hard floor CheckFloor enforces downstream.
set -eu

command -v jq >/dev/null 2>&1 || exit 0
[ -n "${PF_ROUTING_RULES:-}" ] && [ -f "${PF_ROUTING_RULES}" ] || exit 0

payload="$(cat)"
tool_name="$(printf '%s' "${payload}" | jq -r '.tool_name // empty' 2>/dev/null)" || exit 0
[ -n "${tool_name}" ] || exit 0
session_id="$(printf '%s' "${payload}" | jq -r '.session_id // empty' 2>/dev/null || true)"
command_str="$(printf '%s' "${payload}" | jq -r '.tool_input.command // empty' 2>/dev/null || true)"

# matches_prefix CMD PREFIX -- CMD equals PREFIX, or starts with PREFIX
# followed by a space. Mirrors plugin-foundation's Go hasCommandPrefix so
# the live hook and the adoption-measuring registry never disagree about
# what counts as a match.
matches_prefix() {
  cmd="$1"
  prefix="$2"
  [ "${cmd}" = "${prefix}" ] && return 0
  case "${cmd}" in
  "${prefix} "*) return 0 ;;
  esac
  return 1
}

# raw_matches TOOL_NAME COMMAND OP_RAW_JSON -- true iff this invocation is a
# raw usage this operation's CLI supersedes.
#
# POSIX sh functions have no locals, so this scan's loop counter is named
# distinctly from the caller's (rm_i, not i): the outer operations loop below
# also counts with `i`, and a shared name here would have the caller's index
# reset every time this scan finds no match, looping the caller forever.
raw_matches() {
  raw_tool="$(printf '%s' "$3" | jq -r '.tool_name')"
  [ "${raw_tool}" = "$1" ] || return 1
  [ "${raw_tool}" = "Bash" ] || return 0
  prefix_count="$(printf '%s' "$3" | jq -r '.command_prefixes // [] | length')"
  [ "${prefix_count}" -eq 0 ] && return 0
  rm_i=0
  while [ "${rm_i}" -lt "${prefix_count}" ]; do
    p="$(printf '%s' "$3" | jq -r --argjson i "${rm_i}" '.command_prefixes[$i]')"
    matches_prefix "$2" "${p}" && return 0
    rm_i=$((rm_i + 1))
  done
  return 1
}

# cli_bin_path BIN_ENV BIN_NAME -- prints the resolved absolute path of an
# available CLI (BIN_ENV's value if it names an executable file, else
# `command -v` BIN_NAME), or nothing (and a nonzero exit) when unavailable.
cli_bin_path() {
  bin_env="$1"
  bin_name="$2"
  if [ -n "${bin_env}" ]; then
    eval "candidate=\${${bin_env}:-}"
    if [ -n "${candidate}" ] && [ -x "${candidate}" ]; then
      printf '%s' "${candidate}"
      return 0
    fi
  fi
  if [ -n "${bin_name}" ] && command -v "${bin_name}" >/dev/null 2>&1; then
    command -v "${bin_name}"
    return 0
  fi
  return 1
}

log_eval() {
  # $1 operation, $2 outcome
  [ -n "${PF_HOOKEVAL_LOG:-}" ] || return 0
  jq -cn \
    --arg session_id "${session_id}" \
    --arg tool_name "${tool_name}" \
    --arg operation "$1" \
    --arg outcome "$2" \
    '{session_id:$session_id,tool_name:$tool_name,operation:$operation,outcome:$outcome,denies_tool_exists:false}' \
    >>"${PF_HOOKEVAL_LOG}"
}

emit_deny() {
  # $1 reason
  jq -cn --arg r "$1" \
    '{hookSpecificOutput:{hookEventName:"PreToolUse",permissionDecision:"deny",permissionDecisionReason:$r}}'
}

op_count="$(jq -r '.operations | length' "${PF_ROUTING_RULES}" 2>/dev/null || echo 0)"
i=0
while [ "${i}" -lt "${op_count}" ]; do
  op="$(jq -c --argjson i "${i}" '.operations[$i]' "${PF_ROUTING_RULES}")"
  i=$((i + 1))

  op_name="$(printf '%s' "${op}" | jq -r '.name')"
  raw="$(printf '%s' "${op}" | jq -c '.raw')"
  raw_tool="$(printf '%s' "${raw}" | jq -r '.tool_name')"
  [ "${raw_tool}" = "${tool_name}" ] || continue

  if ! raw_matches "${tool_name}" "${command_str}" "${raw}"; then
    log_eval "${op_name}" "not_applicable"
    continue
  fi

  bin_env="$(printf '%s' "${op}" | jq -r '.cli.bin_env // empty')"
  bin_name="$(printf '%s' "${op}" | jq -r '.cli.bin_name // empty')"
  usage_hint="$(printf '%s' "${op}" | jq -r '.cli.usage_hint // empty')"

  if bin_path="$(cli_bin_path "${bin_env}" "${bin_name}")"; then
    log_eval "${op_name}" "fired"
    emit_deny "forced-use: '${op_name}' is governed by ${bin_name} (available at ${bin_path}). Use \`${usage_hint}\` instead of this raw invocation."
    exit 0
  fi

  log_eval "${op_name}" "failed_open"
  exit 0
done

exit 0
