package export_wit_world

import "fmt"

// Greet implements the `greet` export declared in wit/<plugin>.wit.
func Greet(name string) string {
	return fmt.Sprintf("Hello, %s! (from Go)", name)
}
