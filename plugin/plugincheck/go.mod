module github.com/johnrichter/navigator/plugin/plugincheck

go 1.26

require (
	github.com/johnrichter/claude-shared-tooling/go/adoption v0.0.0
	github.com/johnrichter/claude-shared-tooling/go/transcript v0.0.0
	github.com/johnrichter/claude-shared-tooling/plugin-foundation v0.0.0
)

require (
	github.com/gowebpki/jcs v1.0.1 // indirect
	github.com/johnrichter/claude-shared-tooling/go/clikit v0.0.0 // indirect
	github.com/johnrichter/claude-shared-tooling/go/gate v0.0.0 // indirect
	github.com/johnrichter/claude-shared-tooling/go/logkit v0.0.0 // indirect
	github.com/mattn/go-colorable v0.1.14 // indirect
	github.com/mattn/go-isatty v0.0.20 // indirect
	github.com/rs/zerolog v1.35.1 // indirect
	golang.org/x/sys v0.29.0 // indirect
)

// adoption, transcript, plugin-foundation, clikit, gate and logkit are
// ai-shared-lib sibling-repo modules (../../../ai-shared-lib/go/* and
// ../../../ai-shared-lib/plugin-foundation, three levels up from this
// module at navigator/plugin/plugincheck), not yet independently tagged --
// this placeholder version + local replace is a monorepo-development
// stand-in a future release transaction resolves by cutting real tags and
// pointing these requires at them. A `replace` directive is only honored in
// the MAIN module's own go.mod, so the full transitive closure is replaced
// here too.
replace github.com/johnrichter/claude-shared-tooling/go/adoption => ../../../ai-shared-lib/go/adoption

replace github.com/johnrichter/claude-shared-tooling/go/clikit => ../../../ai-shared-lib/go/clikit

replace github.com/johnrichter/claude-shared-tooling/go/gate => ../../../ai-shared-lib/go/gate

replace github.com/johnrichter/claude-shared-tooling/go/logkit => ../../../ai-shared-lib/go/logkit

replace github.com/johnrichter/claude-shared-tooling/go/transcript => ../../../ai-shared-lib/go/transcript

replace github.com/johnrichter/claude-shared-tooling/plugin-foundation => ../../../ai-shared-lib/plugin-foundation
