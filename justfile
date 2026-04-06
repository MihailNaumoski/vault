# Multi-Team Agentic Coding
# ========================

vault := env_var_or_default("VAULT", env_var("HOME") + "/projects/vault/multi-team")
provider := "anthropic"
opus := "claude-opus-4-6"
sonnet := "claude-sonnet-4-6"

# ─── System ───────────────────────────────────────

# Launch the orchestrator — your daily driver
team *task:
    cd {{vault}} && pi --provider {{provider}} --model {{opus}}:xhigh \
       --system-prompt {{vault}}/agents/orchestrator.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read

# List all teams
teams:
    @for d in {{vault}}/agents/*/; do \
      [ -f "$d/lead.md" ] && echo "  - $(basename $d)"; \
    done

# List all agents with their tiers
agents:
    @echo "Orchestrator:"
    @grep -m1 'name:' {{vault}}/agents/orchestrator.md | sed 's/.*name: /  - /'
    @echo ""
    @for d in {{vault}}/agents/*/; do \
      [ -f "$d/lead.md" ] || continue; \
      team=$(basename $d); \
      echo "$team:"; \
      for f in $d/*.md; do \
        echo "$f" | grep -q "expertise" && continue; \
        name=$(grep -m1 'name:' $f | sed 's/.*name: //'); \
        model=$(grep -m1 'model:' $f | sed 's/.*model: //'); \
        echo "  - $name ($model)"; \
      done; \
      echo ""; \
    done

# Show all expertise sizes
stats:
    @echo "Mental Model Sizes:"
    @echo "──────────────────────────"
    @for f in $(command find {{vault}}/agents -name "*-expertise.md" | sort); do \
      name=$(basename ${f%-expertise.md}); \
      lines=$(wc -l < $f | tr -d ' '); \
      words=$(wc -w < $f | tr -d ' '); \
      printf "  %-25s %5s lines  %6s words\n" "$name" "$lines" "$words"; \
    done

# Dream — review all expertise in one view
dream:
    @echo "=== Dream Mode: Expertise Review ==="
    @echo ""
    @for f in $(command find {{vault}}/agents -name "*-expertise.md" | sort); do \
      echo "--- $(basename ${f%-expertise.md}) ---"; \
      cat $f; \
      echo ""; \
    done

# Reset ALL expertise (nuclear option)
reset-all:
    @read -p "RESET ALL MENTAL MODELS? This cannot be undone. [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      for f in $(command find {{vault}}/agents -name "*-expertise.md"); do \
        name=$(basename ${f%-expertise.md}); \
        printf "# %s Expertise\n\n*Fresh start.*\n" "$name" > $f; \
      done; \
      echo "All expertise reset."; \
    fi

# Symlink multi-team into a code project
link project_path:
    mkdir -p {{project_path}}/.pi
    ln -sf {{vault}}/skills {{project_path}}/.pi/skills
    ln -sf {{vault}}/agents {{project_path}}/.pi/agents
    ln -sf {{vault}}/prompts {{project_path}}/.pi/prompts
    @echo "Linked to {{project_path}}"

# Create a new session directory
new-session name="session":
    #!/usr/bin/env bash
    session_id="{{name}}-$(date +%Y%m%d-%H%M%S)"
    dir="{{vault}}/sessions/${session_id}"
    mkdir -p "${dir}"
    touch "${dir}/conversation.jsonl"
    echo "Session: ${dir}"

# Quick test — ping all agent files exist
ping:
    @echo "=== Pinging Agents ==="
    @test -f {{vault}}/agents/orchestrator.md && echo "  ✓ orchestrator" || echo "  ✗ orchestrator MISSING"
    @for team in planning engineering validation; do \
        test -f "{{vault}}/agents/$team/lead.md" && echo "  ✓ $team lead" || echo "  ✗ $team lead MISSING"; \
        for f in {{vault}}/agents/$team/*.md; do \
          echo "$f" | grep -q "expertise" && continue; \
          echo "$f" | grep -q "lead.md" && continue; \
          name=$(basename ${f%.md}); \
          test -f "$f" && echo "  ✓ $team/$name" || echo "  ✗ $team/$name MISSING"; \
        done; \
    done

# Login to Anthropic (run once)
login:
    pi --provider {{provider}}

# ─── Tmux Layouts ─────────────────────────────────

# Launch orchestrator in tmux
tmux-team layout="orchestrator":
    {{vault}}/tmux-team.sh {{layout}}

# Launch all teams in tmux (4 windows: orchestrator + planning + engineering + validation)
tmux-all:
    {{vault}}/tmux-team.sh all

# Launch orchestrator + project shell side by side
tmux-project path=".":
    {{vault}}/tmux-team.sh project {{path}}

# ─── Planning Team ────────────────────────────────

planning:
    pi --provider {{provider}} --model {{opus}}:xhigh \
       --system-prompt {{vault}}/agents/planning/lead.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read

planning-run *task:
    pi --provider {{provider}} --model {{opus}}:xhigh \
       --system-prompt {{vault}}/agents/planning/lead.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read \
       -p "{{task}}"

planning-expertise:
    @echo "=== Planning Lead ==="
    @cat {{vault}}/agents/planning/lead-expertise.md
    @echo ""
    @for f in {{vault}}/agents/planning/*-expertise.md; do \
      [ "$(basename $f)" = "lead-expertise.md" ] && continue; \
      echo "=== $(basename ${f%-expertise.md}) ==="; \
      cat $f; \
      echo ""; \
    done

planning-reset:
    @read -p "Reset ALL mental models for planning? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      for f in {{vault}}/agents/planning/*-expertise.md; do \
        name=$(basename ${f%-expertise.md}); \
        printf "# %s Expertise\n\n*Fresh start.*\n" "$name" > $f; \
      done; \
      echo "Planning team reset."; \
    fi

planning-members:
    @echo "Planning Team:"
    @for f in {{vault}}/agents/planning/*.md; do \
      echo "$f" | grep -q "expertise" && continue; \
      grep -m1 "^name:" $f 2>/dev/null | sed 's/name: /  - /'; \
    done

# ─── Architect ────────────────────────────────────

architect:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/planning/architect.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,write,edit,bash

architect-run *task:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/planning/architect.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,write,edit,bash \
       -p "{{task}}"

architect-expertise:
    @cat {{vault}}/agents/planning/architect-expertise.md

architect-reset:
    @read -p "Reset Architect's mental model? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      printf "# Architect Expertise\n\n*Fresh start.*\n" > {{vault}}/agents/planning/architect-expertise.md; \
      echo "Reset complete."; \
    fi

architect-stats:
    @echo "Architect:"
    @wc -l < {{vault}}/agents/planning/architect-expertise.md | xargs -I{} echo "  Lines: {}"
    @wc -w < {{vault}}/agents/planning/architect-expertise.md | xargs -I{} echo "  Words: {}"

# ─── Spec Writer ──────────────────────────────────

spec-writer:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/planning/spec-writer.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,write,edit,bash

spec-writer-run *task:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/planning/spec-writer.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,write,edit,bash \
       -p "{{task}}"

spec-writer-expertise:
    @cat {{vault}}/agents/planning/spec-writer-expertise.md

spec-writer-reset:
    @read -p "Reset Spec Writer's mental model? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      printf "# Spec Writer Expertise\n\n*Fresh start.*\n" > {{vault}}/agents/planning/spec-writer-expertise.md; \
      echo "Reset complete."; \
    fi

spec-writer-stats:
    @echo "Spec Writer:"
    @wc -l < {{vault}}/agents/planning/spec-writer-expertise.md | xargs -I{} echo "  Lines: {}"
    @wc -w < {{vault}}/agents/planning/spec-writer-expertise.md | xargs -I{} echo "  Words: {}"

# ─── Engineering Team ─────────────────────────────

engineering:
    pi --provider {{provider}} --model {{opus}}:xhigh \
       --system-prompt {{vault}}/agents/engineering/lead.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read

engineering-run *task:
    pi --provider {{provider}} --model {{opus}}:xhigh \
       --system-prompt {{vault}}/agents/engineering/lead.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read \
       -p "{{task}}"

engineering-expertise:
    @echo "=== Engineering Lead ==="
    @cat {{vault}}/agents/engineering/lead-expertise.md
    @echo ""
    @for f in {{vault}}/agents/engineering/*-expertise.md; do \
      [ "$(basename $f)" = "lead-expertise.md" ] && continue; \
      echo "=== $(basename ${f%-expertise.md}) ==="; \
      cat $f; \
      echo ""; \
    done

engineering-reset:
    @read -p "Reset ALL mental models for engineering? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      for f in {{vault}}/agents/engineering/*-expertise.md; do \
        name=$(basename ${f%-expertise.md}); \
        printf "# %s Expertise\n\n*Fresh start.*\n" "$name" > $f; \
      done; \
      echo "Engineering team reset."; \
    fi

engineering-members:
    @echo "Engineering Team:"
    @for f in {{vault}}/agents/engineering/*.md; do \
      echo "$f" | grep -q "expertise" && continue; \
      grep -m1 "^name:" $f 2>/dev/null | sed 's/name: /  - /'; \
    done

# ─── Backend Dev ──────────────────────────────────

backend-dev:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/engineering/backend-dev.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,write,edit,bash

backend-dev-run *task:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/engineering/backend-dev.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,write,edit,bash \
       -p "{{task}}"

backend-dev-expertise:
    @cat {{vault}}/agents/engineering/backend-dev-expertise.md

backend-dev-reset:
    @read -p "Reset Backend Dev's mental model? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      printf "# Backend Dev Expertise\n\n*Fresh start.*\n" > {{vault}}/agents/engineering/backend-dev-expertise.md; \
      echo "Reset complete."; \
    fi

backend-dev-stats:
    @echo "Backend Dev:"
    @wc -l < {{vault}}/agents/engineering/backend-dev-expertise.md | xargs -I{} echo "  Lines: {}"
    @wc -w < {{vault}}/agents/engineering/backend-dev-expertise.md | xargs -I{} echo "  Words: {}"

# ─── Frontend Dev ─────────────────────────────────

frontend-dev:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/engineering/frontend-dev.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,write,edit,bash

frontend-dev-run *task:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/engineering/frontend-dev.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,write,edit,bash \
       -p "{{task}}"

frontend-dev-expertise:
    @cat {{vault}}/agents/engineering/frontend-dev-expertise.md

frontend-dev-reset:
    @read -p "Reset Frontend Dev's mental model? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      printf "# Frontend Dev Expertise\n\n*Fresh start.*\n" > {{vault}}/agents/engineering/frontend-dev-expertise.md; \
      echo "Reset complete."; \
    fi

frontend-dev-stats:
    @echo "Frontend Dev:"
    @wc -l < {{vault}}/agents/engineering/frontend-dev-expertise.md | xargs -I{} echo "  Lines: {}"
    @wc -w < {{vault}}/agents/engineering/frontend-dev-expertise.md | xargs -I{} echo "  Words: {}"

# ─── Validation Team ─────────────────────────────

validation:
    pi --provider {{provider}} --model {{opus}}:xhigh \
       --system-prompt {{vault}}/agents/validation/lead.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read

validation-run *task:
    pi --provider {{provider}} --model {{opus}}:xhigh \
       --system-prompt {{vault}}/agents/validation/lead.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read \
       -p "{{task}}"

validation-expertise:
    @echo "=== Validation Lead ==="
    @cat {{vault}}/agents/validation/lead-expertise.md
    @echo ""
    @for f in {{vault}}/agents/validation/*-expertise.md; do \
      [ "$(basename $f)" = "lead-expertise.md" ] && continue; \
      echo "=== $(basename ${f%-expertise.md}) ==="; \
      cat $f; \
      echo ""; \
    done

validation-reset:
    @read -p "Reset ALL mental models for validation? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      for f in {{vault}}/agents/validation/*-expertise.md; do \
        name=$(basename ${f%-expertise.md}); \
        printf "# %s Expertise\n\n*Fresh start.*\n" "$name" > $f; \
      done; \
      echo "Validation team reset."; \
    fi

validation-members:
    @echo "Validation Team:"
    @for f in {{vault}}/agents/validation/*.md; do \
      echo "$f" | grep -q "expertise" && continue; \
      grep -m1 "^name:" $f 2>/dev/null | sed 's/name: /  - /'; \
    done

# ─── QA Engineer ──────────────────────────────────

qa-engineer:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/validation/qa-engineer.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,bash

qa-engineer-run *task:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/validation/qa-engineer.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,bash \
       -p "{{task}}"

qa-engineer-expertise:
    @cat {{vault}}/agents/validation/qa-engineer-expertise.md

qa-engineer-reset:
    @read -p "Reset QA Engineer's mental model? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      printf "# QA Engineer Expertise\n\n*Fresh start.*\n" > {{vault}}/agents/validation/qa-engineer-expertise.md; \
      echo "Reset complete."; \
    fi

qa-engineer-stats:
    @echo "QA Engineer:"
    @wc -l < {{vault}}/agents/validation/qa-engineer-expertise.md | xargs -I{} echo "  Lines: {}"
    @wc -w < {{vault}}/agents/validation/qa-engineer-expertise.md | xargs -I{} echo "  Words: {}"

# ─── Security Reviewer ───────────────────────────

security-reviewer:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/validation/security-reviewer.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,grep,find

security-reviewer-run *task:
    pi --provider {{provider}} --model {{sonnet}}:high \
       --system-prompt {{vault}}/agents/validation/security-reviewer.md \
       --skill {{vault}}/skills \
       --session-dir {{vault}}/sessions \
       --tools read,grep,find \
       -p "{{task}}"

security-reviewer-expertise:
    @cat {{vault}}/agents/validation/security-reviewer-expertise.md

security-reviewer-reset:
    @read -p "Reset Security Reviewer's mental model? [y/N] " confirm; \
    if [ "$confirm" = "y" ]; then \
      printf "# Security Reviewer Expertise\n\n*Fresh start.*\n" > {{vault}}/agents/validation/security-reviewer-expertise.md; \
      echo "Reset complete."; \
    fi

security-reviewer-stats:
    @echo "Security Reviewer:"
    @wc -l < {{vault}}/agents/validation/security-reviewer-expertise.md | xargs -I{} echo "  Lines: {}"
    @wc -w < {{vault}}/agents/validation/security-reviewer-expertise.md | xargs -I{} echo "  Words: {}"
