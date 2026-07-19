---
name: "Projects directory"
description: "Convention for in-repo, short-lived project workspaces under .projects/<slug>/ — tracked in git so delivery-agent-team (build-with-team) can plan, build, resume, and release in-repo."
id: "doc:navigator:projects-convention"
tags: [type:doc, topic:process, status:complete, privacy:public, owner:public]
links: []
updated: 2026-07-19T00:00:00Z
---

# Projects

Short-lived, in-repo project workspaces — one directory per project (`.projects/<slug>/`).

- **Tracked in git.** Projects are committed so `delivery-agent-team` (build-with-team) can plan, build, resume, and hand off across sessions.
- **Short-lived.** Complete the work, cut a release (git tag / GitHub artifact / plugin version), consume it downstream, then prune the project directory.
- **Scope.** Everything a project needs (design, plan, notes, scratch) lives under its slug directory — not scattered across the repo root.
