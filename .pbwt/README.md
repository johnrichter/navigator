---
name: "Plan/build-with-team directory"
description: "In-repo home for plan-with-team / build-with-team (PBWT) plans and builds — one directory per effort under .pbwt/<slug>/, tracked in git so delivery-agent-team can plan, build, resume, and release in-repo. Transitional: to be retired in favor of .anoikis/."
id: "doc:navigator:pbwt-convention"
tags: [type:doc, topic:process, status:complete, privacy:public, owner:public]
links: [doc:navigator:anoikis-convention]
updated: 2026-07-19T00:00:00Z
---

# .pbwt — plan/build-with-team

Home for `delivery-agent-team` **plan-with-team** and **build-with-team** (PBWT) work — one directory per effort (`.pbwt/<slug>/`).

- **Tracked in git.** Plans and builds are committed so the harness can plan, build, resume, and hand off across sessions.
- **Short-lived.** Complete the work, cut a release (git tag / GitHub artifact / plugin version), consume it downstream, then prune the effort directory.
- **Scope.** Everything an effort needs (design, plan, notes, scratch) lives under its slug directory — not scattered across the repo root.

> **Transitional.** `.pbwt/` will be retired in favor of `.anoikis/` (reserved now) once anoikis-based planning and builds take over. New efforts will move there.
