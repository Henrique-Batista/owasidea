package export_wit_world

import (
	"fmt"
	"sort"
	"strings"

	poc_host_host_log "wit_component/poc_host_host_log"
)

func Scan(dependencies []string) string {
	items := append([]string(nil), dependencies...)
	sort.Strings(items)

	findings := make([]string, 0)
	for _, dep := range items {
		lower := strings.ToLower(strings.TrimSpace(dep))
		if lower == "" {
			continue
		}
		switch {
		case strings.Contains(lower, "alpha"):
			findings = append(findings, dep+" is a pre-release (alpha)")
		case strings.Contains(lower, "beta"):
			findings = append(findings, dep+" is a pre-release (beta)")
		case strings.Contains(lower, "rc."):
			findings = append(findings, dep+" is a release candidate")
		case strings.Contains(lower, "0.0."):
			findings = append(findings, dep+" has an unstable 0.0.x version")
		}
	}

	poc_host_host_log.Log(fmt.Sprintf("audit-go scanned %d dependencies", len(items)))
	if len(findings) == 0 {
		return fmt.Sprintf("audited %d dependencies; no pre-release flags", len(items))
	}
	return fmt.Sprintf("audited %d dependencies; %d need attention: %s", len(items), len(findings), strings.Join(findings, "; "))
}
