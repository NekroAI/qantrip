---
name: _scope
description: 公共片段：改动范围
---
{% if scope == "staged" %}暂存区的改动（git diff --cached）{% elif scope == "branch" %}当前分支相对主分支的全部改动{% elif scope == "last" %}最近一次提交（git show HEAD）{% else %}当前所有未提交的改动（git diff HEAD）{% endif %}
