package export_wit_world

import (
	"fmt"

	poc_host_host_kv "wit_component/poc_host_host_kv"
	poc_host_host_log "wit_component/poc_host_host_log"
)

// owasidea:world attr-go
// owasidea:host_log
// owasidea:host_kv

// owasidea:export
func Greet(name string) string {
	poc_host_host_log.Log(fmt.Sprintf("greet called with %s", name))
	return fmt.Sprintf("Hello, %s! (from attr-go)", name)
}

// owasidea:export
func Compute(a int32, b int32) int32 {
	poc_host_host_log.Log(fmt.Sprintf("compute %d + %d", a, b))
	return a + b
}

// owasidea:export
func Remember(key string, value string) string {
	poc_host_host_kv.Set(key, value)
	poc_host_host_log.Log(fmt.Sprintf("remembered %s", key))
	return fmt.Sprintf("stored %s", key)
}
