package main

import (
	"encoding/json"
	"io"
	"net/http"
	"strings"

	"go.bytecodealliance.org/pkg/wasihttp"
	_ "go.wasmcloud.dev/component"
)

type chatRequest struct {
	Message string `json:"message"`
	User    string `json:"user"`
}

type chatResponse struct {
	Reply string `json:"reply"`
}

func init() {
	wasihttp.HandleFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("content-type", "application/json")

		// Outbound HTTP demo: /?url=https://example.com
		if target := r.URL.Query().Get("url"); target != "" {
			resp, err := wasihttp.DefaultClient.Get(target)
			if err != nil {
				w.WriteHeader(http.StatusBadGateway)
				_ = json.NewEncoder(w).Encode(map[string]string{"error": err.Error()})
				return
			}
			defer resp.Body.Close()
			body, _ := io.ReadAll(io.LimitReader(resp.Body, 4096))
			_ = json.NewEncoder(w).Encode(map[string]any{
				"status": resp.StatusCode,
				"body":   string(body),
			})
			return
		}

		var req chatRequest
		if r.Body != nil {
			defer r.Body.Close()
			_ = json.NewDecoder(r.Body).Decode(&req)
		}
		if req.User == "" {
			req.User = "anonymous"
		}
		reply := "chat-go heard: " + strings.TrimSpace(req.Message)
		if req.Message == "" {
			reply = "chat-go is alive (POST a JSON body with a message)"
		}
		_ = json.NewEncoder(w).Encode(chatResponse{Reply: reply})
	})
}

func main() {}
