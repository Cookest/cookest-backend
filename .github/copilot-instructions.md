<!-- This file is read automatically by GitHub Copilot in VS Code. -->
<!-- Full instructions are in AGENTS.md in this directory. -->

@AGENTS.md

<!-- For deep context on any specific role, the parent workspace has: -->
<!-- ../agents/api-agent.md  — Rust API deep-dive -->
<!-- ../agents/ui-agent.md   — Flutter + landing page -->
<!-- ../agents/docs-agent.md — Documentation writing -->
<!-- ../agents/planner-agent.md — Cross-repo task planning -->
<!-- ../agents/eval-agent.md — Code review and QA checklists -->
<!-- ../AGENTS.md            — Full project map and universal rules -->

<!-- ── Context7 Rule ──────────────────────────────────────────────── -->
<!-- Before implementing with ANY external library, use Context7 to   -->
<!-- fetch current version-accurate docs. Two steps:                  -->
<!--   1. resolve-library-id({ libraryName, query }) — get the ID     -->
<!--   2. query-docs({ libraryId, query }) — fetch the docs           -->
<!-- Or use a known ID from vault/Learnings/library-ids.md directly.  -->
<!-- In chat: prefix with "use context7" or "use library /id/here"    -->
<!-- This prevents hallucinated APIs and outdated code patterns.       -->
