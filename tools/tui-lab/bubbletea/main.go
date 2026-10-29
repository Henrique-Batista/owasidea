package main

import (
	"fmt"
	"os"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
)

var (
	titleStyle = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("51"))
	tabStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("245"))
	activeTab  = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("220"))
	errorStyle = lipgloss.NewStyle().Background(lipgloss.Color("160")).Foreground(lipgloss.Color("231")).Padding(0, 1)
	okStyle    = lipgloss.NewStyle().Foreground(lipgloss.Color("42"))
	failStyle  = lipgloss.NewStyle().Foreground(lipgloss.Color("196")).Bold(true)
	dimStyle   = lipgloss.NewStyle().Foreground(lipgloss.Color("240"))
	boxStyle   = lipgloss.NewStyle().Border(lipgloss.RoundedBorder()).Padding(0, 1)
)

var tabs = []string{"Overview", "Plugins", "Logs", "Metrics", "Tests"}

type plugin struct {
	name string
	kind string
	ok   bool
}

type logLine struct {
	time, level, msg string
}

type testLine struct {
	name   string
	ok     bool
	detail string
}

type model struct {
	tab      int
	buildErr string
	plugins  []plugin
	logs     []logLine
	tests    []testLine
	frame    int
}

func initialModel() model {
	return model{
		buildErr: "plugins/audit-go: componentize-go failed (exit 1) — see Logs",
		plugins: []plugin{
			{"core-rust", "library", true},
			{"audit-go", "library", false},
			{"watchtower-rust", "service", true},
			{"board-svelte", "ui", true},
		},
		logs: []logLine{
			{"12:04:01", "INFO", "host ready on 127.0.0.1:8787"},
			{"12:04:02", "INFO", "loaded core-rust (wasmtime 50)"},
			{"12:04:03", "WARN", "audit-go: rebuild failed, keeping previous component"},
			{"12:04:03", "ERROR", "build: exit 1; last output: go: no such tool componentize-go"},
			{"12:04:04", "INFO", "service watchtower-rust started"},
		},
		tests: []testLine{
			{"smoke:greet", true, "3 ms"},
			{"smoke:sum", true, "2 ms"},
			{"ui:round-trip", false, "timeout 30s"},
		},
	}
}

func (m model) Init() tea.Cmd { return nil }

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	if key, ok := msg.(tea.KeyPressMsg); ok {
		switch key.String() {
		case "q", "ctrl+c":
			return m, tea.Quit
		case "r":
			m.buildErr = ""
		case "1", "2", "3", "4", "5":
			m.tab = int(key.String()[0] - '1')
		case "left", "h":
			if m.tab > 0 {
				m.tab--
			}
		case "right", "l":
			if m.tab < len(tabs)-1 {
				m.tab++
			}
		}
	}
	return m, nil
}

func (m model) View() tea.View {
	var b strings.Builder
	b.WriteString(titleStyle.Render("Owasidea Dev"))
	b.WriteString("  build #128  ")
	b.WriteString(okStyle.Render("● host"))
	b.WriteString("\n")

	for i, tab := range tabs {
		if i == m.tab {
			b.WriteString(activeTab.Render(" " + tab + " "))
		} else {
			b.WriteString(tabStyle.Render(" " + tab + " "))
		}
		b.WriteString("│")
	}
	b.WriteString("\n\n")

	if m.buildErr != "" {
		b.WriteString(errorStyle.Render(" BUILD FAILED  " + m.buildErr))
		b.WriteString("\n\n")
	}

	switch m.tab {
	case 0:
		b.WriteString(m.overview())
	case 1:
		b.WriteString(m.pluginsView())
	case 2:
		b.WriteString(m.logsView())
	case 3:
		b.WriteString(m.metricsView())
	case 4:
		b.WriteString(m.testsView())
	}

	b.WriteString("\n")
	b.WriteString(dimStyle.Render("1-5 tabs  ←/→ move  r rebuild  q quit"))
	return tea.NewView(b.String())
}

func (m model) overview() string {
	left := boxStyle.Width(34).Render(strings.Join([]string{
		"App      hello",
		"Plugins  4 loaded, 1 failing",
		"Services 1 running",
		"Build    128 · 8.9s",
		"",
		activeTab.Render("Next steps"),
		"  fix audit-go build",
		"  press r to retry",
	}, "\n"))

	var right strings.Builder
	right.WriteString(dimStyle.Render("Latest logs") + "\n")
	for _, l := range m.logs {
		right.WriteString(l.time + " " + level(l.level) + " " + l.msg + "\n")
	}
	return lipgloss.JoinHorizontal(lipgloss.Top, left, "  ", boxStyle.Width(62).Render(right.String()))
}

func (m model) pluginsView() string {
	var b strings.Builder
	for _, p := range m.plugins {
		mark := okStyle.Render("●")
		status := okStyle.Render("loaded")
		if !p.ok {
			mark = failStyle.Render("✕")
			status = failStyle.Render("error")
		}
		fmt.Fprintf(&b, "%s %-18s %-8s %s\n", mark, p.name, p.kind, status)
	}
	return boxStyle.Width(60).Render(b.String())
}

func (m model) logsView() string {
	var b strings.Builder
	for _, l := range m.logs {
		fmt.Fprintf(&b, "%s %-5s %s\n", dimStyle.Render(l.time), level(l.level), l.msg)
	}
	return boxStyle.Width(90).Render(b.String())
}

func (m model) metricsView() string {
	return boxStyle.Width(90).Render(strings.Join([]string{
		"core-rust p95 latency   1.1 ms / 20 ms   " + okStyle.Render("████░░░░░░░░░░░░░░░░"),
		"host memory            42 MB / 256 MB    " + activeTab.Render("███░░░░░░░░░░░░░░░░░"),
		"",
		"requests  1 284   errors 3   fuel 12%",
		"calls/s   41.2    p50 0.9 ms   p95 1.1 ms",
		"top       core-rust 72% · changelog-ts 18% · audit-go 10%",
	}, "\n"))
}

func (m model) testsView() string {
	var b strings.Builder
	for _, t := range m.tests {
		status := okStyle.Render("PASS")
		if !t.ok {
			status = failStyle.Render("FAIL")
		}
		fmt.Fprintf(&b, "%s %-20s %s\n", status, t.name, t.detail)
	}
	return boxStyle.Width(60).Render(b.String())
}

func level(l string) string {
	switch l {
	case "ERROR":
		return failStyle.Render("ERROR")
	case "WARN":
		return activeTab.Render("WARN ")
	default:
		return tabStyle.Render("INFO ")
	}
}

func main() {
	p := tea.NewProgram(initialModel())
	if _, err := p.Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
