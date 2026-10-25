package export_wit_world

import (
	"fmt"

	poc_notes_api "wit_component/poc_notes_api"
)

// owasidea:world composed-go
// owasidea:host_log? no

// owasidea:export
func Run(name string) string {
	g := poc_notes_api.Greet(name)
	return fmt.Sprintf("composed-go client got: %s", g)
}
