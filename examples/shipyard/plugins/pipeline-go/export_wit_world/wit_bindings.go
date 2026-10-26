package export_wit_world

import (
	"fmt"

	core "wit_component/shipyard_core_readiness"
)

// Run imports the Rust readiness component. `wasm-tools compose` links the
// implementation into this component at build time.
func Run(version string, changes []string, checksPassed uint32, checksTotal uint32) string {
	score := core.Score(changes, checksPassed, checksTotal)
	verdict := core.Verdict(score)
	return fmt.Sprintf("composed Go→Rust pipeline: %s scored %d (%s) across %d changes", version, score, verdict, len(changes))
}
