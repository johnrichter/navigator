// Package plugincheck is the build-acceptance test for navigator's own plugin: its
// routing-rules.json, loaded through plugin-foundation's shared registry (never a
// hand-written Classify closure), must clear the Phase-A adoption gate on the frozen
// fixture transcript for every governed operation it declares.
package plugincheck

import (
	"testing"

	plugin_foundation "github.com/johnrichter/claude-shared-tooling/plugin-foundation"

	"github.com/johnrichter/claude-shared-tooling/go/adoption"
	"github.com/johnrichter/claude-shared-tooling/go/transcript"
)

func TestRoutingRulesClearThePhaseAGateOnFrozenFixtures(t *testing.T) {
	rules, err := plugin_foundation.LoadRoutingRulesFile("../routing-rules.json")
	if err != nil {
		t.Fatalf("LoadRoutingRulesFile: %v", err)
	}
	registry := plugin_foundation.BuildRegistry(rules)
	if len(registry) != len(rules.Operations) {
		t.Fatalf("BuildRegistry produced %d operations, want %d", len(registry), len(rules.Operations))
	}

	source := transcript.ClaudeCodeJSONL{}
	invocations, err := adoption.LoadSessionInvocations(source, "testdata/transcripts", "proj", "session-a")
	if err != nil {
		t.Fatalf("LoadSessionInvocations: %v", err)
	}

	classifications := adoption.Classify(registry, invocations)
	rates, err := adoption.Rate(classifications, adoption.PhaseAStartGatePercent)
	if err != nil {
		t.Fatalf("Rate: %v", err)
	}

	if len(rates) != len(rules.Operations) {
		t.Fatalf("Rate produced %d operations, want %d", len(rates), len(rules.Operations))
	}
	for _, op := range rules.Operations {
		a, ok := rates[op.Name]
		if !ok {
			t.Errorf("operation %q missing from Rate output", op.Name)
			continue
		}
		if a.CLICount != 4 || a.RawCount != 1 {
			t.Errorf("operation %q counts = %d cli, %d raw, want 4, 1", op.Name, a.CLICount, a.RawCount)
		}
		if !a.MetGate() {
			t.Errorf("operation %q adopted its CLI at %.2f%%, below the %d%% Phase-A floor", op.Name, a.Rate*100, adoption.PhaseAStartGatePercent)
		}
	}

	report, err := adoption.BuildReport(classifications, nil, adoption.PhaseAStartGatePercent)
	if err != nil {
		t.Fatalf("BuildReport: %v", err)
	}
	result, err := report.Result([]string{"navigator", "plugin", "adoption"})
	if err != nil {
		t.Fatalf("Report.Result: %v", err)
	}
	if result.Status != "success" {
		t.Errorf("Result.Status = %q, want success (every operation cleared its gate)", result.Status)
	}
}
