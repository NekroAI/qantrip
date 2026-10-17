---
name: _scope
description: "Shared snippet: which changes to look at"
---
{% if scope == "staged" %}the staged changes (git diff --cached){% elif scope == "branch" %}all changes on the current branch relative to the main branch{% elif scope == "last" %}the most recent commit (git show HEAD){% else %}all uncommitted changes (git diff HEAD){% endif %}
