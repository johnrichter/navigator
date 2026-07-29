package plugincheck

import (
	"crypto/sha256"
	"encoding/hex"
	"os"
	"os/exec"
	"strings"
	"testing"
)

// Canonical sha256 digests of ai-shared-lib/plugin-foundation's
// forced-use-hook.sh and download-script.sh, recorded when this plugin last
// synced its vendored copies. A mismatch here means this plugin has forked
// the shared foundation (SC-FORCEDUSE, SC-LIBFIRST): resync the vendored
// file from ai-shared-lib/plugin-foundation instead of hand-editing it.
const (
	canonicalForcedUseHookSHA256  = "d13439b6725e26a0eb24169d8a14d068b1307b6b6bcceba7695363a4dd99fced"
	canonicalDownloadScriptSHA256 = "9a75c12c41f6515d707a87d1d902df30f7b2fceae96242709800ea5fec384a23"
)

func sha256Hex(t *testing.T, path string) string {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func TestVendoredHookScriptsMatchCanonicalDigest(t *testing.T) {
	if got := sha256Hex(t, "../hooks/forced-use-hook.sh"); got != canonicalForcedUseHookSHA256 {
		t.Errorf("plugin/hooks/forced-use-hook.sh sha256 = %s, want %s (resync from ai-shared-lib/plugin-foundation, don't fork)", got, canonicalForcedUseHookSHA256)
	}
	if got := sha256Hex(t, "../hooks/download-script.sh"); got != canonicalDownloadScriptSHA256 {
		t.Errorf("plugin/hooks/download-script.sh sha256 = %s, want %s (resync from ai-shared-lib/plugin-foundation, don't fork)", got, canonicalDownloadScriptSHA256)
	}
}

// TestForcedUseHookTerminatesOnNoMatchAgainstRealRoutingRules guards against
// the hang a stale forced-use-hook.sh reintroduces: scanning multiple
// non-matching Bash operations against this plugin's real routing-rules.json
// must still return promptly, not loop forever on a clobbered scan counter.
func TestForcedUseHookTerminatesOnNoMatchAgainstRealRoutingRules(t *testing.T) {
	cmd := exec.Command("timeout", "5", "../hooks/forced-use-hook.sh")
	cmd.Env = append(os.Environ(), "PF_ROUTING_RULES=../routing-rules.json")
	cmd.Stdin = strings.NewReader(`{"session_id":"s1","tool_name":"Bash","tool_input":{"command":"echo not-a-governed-command"}}`)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("hook did not terminate cleanly within the bounded timeout: %v (output: %s)", err, out)
	}
	if len(out) != 0 {
		t.Errorf("hook emitted output %q for a non-matching command, want silent allow", out)
	}
}
