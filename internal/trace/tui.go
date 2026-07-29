// Copyright 2026 Erst Users
// SPDX-License-Identifier: Apache-2.0

// Package trace provides interactive terminal UI for exploring Soroban
// execution traces. tui.go is the thin entry-point that wires a Model to a
// bubbletea program; all state logic lives in model.go.

package trace

import (
	"fmt"

	tea "github.com/charmbracelet/bubbletea"
)

// RunTUI launches the bubbletea-based interactive trace viewer for the given
// ExecutionTrace. It blocks until the user quits and returns any terminal or
// I/O error encountered during the session.
//
// Usage:
//
//	if err := trace.RunTUI(myTrace); err != nil {
//	    log.Fatal(err)
//	}
func RunTUI(t *ExecutionTrace) error {
	if t == nil {
		return fmt.Errorf("RunTUI: trace must not be nil")
	}

	m := NewModel(t)
	p := tea.NewProgram(
		m,
		tea.WithAltScreen(),       // use the terminal alternate screen buffer
		tea.WithMouseCellMotion(), // enable mouse support for future expansion
	)

	_, err := p.Run()
	return err
}
