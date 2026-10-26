package main

import (
	"encoding/json"
	"net/http"
	"strings"
	"time"

	"go.bytecodealliance.org/pkg/wasihttp"
	_ "go.wasmcloud.dev/component"
)

type event struct {
	Type      string         `json:"type"`
	Component string         `json:"component"`
	Payload   map[string]any `json:"payload"`
}

func init() {
	wasihttp.HandleFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("content-type", "application/json")
		switch {
		case r.URL.Path == "" || r.URL.Path == "/" || strings.HasSuffix(r.URL.Path, "/health"):
			_ = json.NewEncoder(w).Encode(map[string]any{
				"ok":      true,
				"service": "shipyard-webhook",
				"runtime": "wasi:http/incoming-handler",
			})
		case strings.HasSuffix(r.URL.Path, "/events") && r.Method == http.MethodPost:
			var incoming event
			if r.Body != nil {
				defer r.Body.Close()
				_ = json.NewDecoder(r.Body).Decode(&incoming)
			}
			if incoming.Type == "" {
				incoming.Type = "release.observed"
			}
			if incoming.Component == "" {
				incoming.Component = "unknown"
			}
			_ = json.NewEncoder(w).Encode(map[string]any{
				"accepted":    true,
				"event":       incoming,
				"received_at": time.Now().UTC().Format(time.RFC3339),
			})
		default:
			w.WriteHeader(http.StatusNotFound)
			_ = json.NewEncoder(w).Encode(map[string]string{"error": "unknown webhook path"})
		}
	})
}

func main() {}
