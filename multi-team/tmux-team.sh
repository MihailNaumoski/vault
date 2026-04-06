#!/bin/bash
# Launch multi-team agents in tmux panes
# Usage: ./tmux-team.sh [layout]
#   layouts: orchestrator, planning, engineering, validation, all

VAULT="$HOME/projects/vault/multi-team"
JUSTFILE="$HOME/projects/vault/justfile"
J="just --justfile $JUSTFILE"
SESSION="team"
LAYOUT="${1:-orchestrator}"

# Kill existing session
tmux kill-session -t "$SESSION" 2>/dev/null

case "$LAYOUT" in

  # Just the orchestrator (default)
  orchestrator)
    tmux new-session -d -s "$SESSION" -n "orchestrator" "cd $VAULT && $J team"
    ;;

  # Planning team: lead + architect + spec-writer
  planning)
    tmux new-session -d -s "$SESSION" -n "planning" "cd $VAULT && $J planning"
    tmux split-window -h -t "$SESSION" "cd $VAULT && $J architect"
    tmux split-window -v -t "$SESSION:0.1" "cd $VAULT && $J spec-writer"
    tmux select-pane -t "$SESSION:0.0"
    ;;

  # Engineering team: lead + backend + frontend
  engineering)
    tmux new-session -d -s "$SESSION" -n "engineering" "cd $VAULT && $J engineering"
    tmux split-window -h -t "$SESSION" "cd $VAULT && $J backend-dev"
    tmux split-window -v -t "$SESSION:0.1" "cd $VAULT && $J frontend-dev"
    tmux select-pane -t "$SESSION:0.0"
    ;;

  # Validation team: lead + qa + security
  validation)
    tmux new-session -d -s "$SESSION" -n "validation" "cd $VAULT && $J validation"
    tmux split-window -h -t "$SESSION" "cd $VAULT && $J qa-engineer"
    tmux split-window -v -t "$SESSION:0.1" "cd $VAULT && $J security-reviewer"
    tmux select-pane -t "$SESSION:0.0"
    ;;

  # Full team: orchestrator + all 3 teams in separate windows
  all)
    # Window 0: Orchestrator
    tmux new-session -d -s "$SESSION" -n "orchestrator" "cd $VAULT && $J team"

    # Window 1: Planning
    tmux new-window -t "$SESSION" -n "planning" "cd $VAULT && $J planning"
    tmux split-window -h -t "$SESSION:1" "cd $VAULT && $J architect"
    tmux split-window -v -t "$SESSION:1.1" "cd $VAULT && $J spec-writer"

    # Window 2: Engineering
    tmux new-window -t "$SESSION" -n "engineering" "cd $VAULT && $J engineering"
    tmux split-window -h -t "$SESSION:2" "cd $VAULT && $J backend-dev"
    tmux split-window -v -t "$SESSION:2.1" "cd $VAULT && $J frontend-dev"

    # Window 3: Validation
    tmux new-window -t "$SESSION" -n "validation" "cd $VAULT && $J validation"
    tmux split-window -h -t "$SESSION:3" "cd $VAULT && $J qa-engineer"
    tmux split-window -v -t "$SESSION:3.1" "cd $VAULT && $J security-reviewer"

    # Back to orchestrator
    tmux select-window -t "$SESSION:0"
    ;;

  # Project-specific: orchestrator + project dir in split
  project)
    PROJECT="${2:-.}"
    tmux new-session -d -s "$SESSION" -n "work" "cd $VAULT && $J team"
    tmux split-window -h -t "$SESSION" "cd $PROJECT && $SHELL"
    tmux select-pane -t "$SESSION:0.0"
    ;;

  *)
    echo "Usage: $0 [orchestrator|planning|engineering|validation|all|project <path>]"
    exit 1
    ;;
esac

tmux attach -t "$SESSION"
