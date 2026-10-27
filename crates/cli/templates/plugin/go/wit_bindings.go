// owasidea:world {{name}}
package export_wit_world

import "fmt"

// owasidea:export
func Greet(name string) string {
	return fmt.Sprintf("Hello, %s! (from Go)", name)
}
