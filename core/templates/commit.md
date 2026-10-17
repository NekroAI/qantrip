---
name: commit
aliases: [ci]
description: 生成提交信息
category: 编程
params:
  style:
    type: enum
    positional: true
    description: 格式
    default: conventional
    choices:
      - { value: conventional, label: Conventional Commits }
      - { value: plain, label: 普通格式 }
  lang:
    type: enum
    description: 语言
    default: en
    choices:
      - { value: en, label: 英文 }
      - { value: zh, label: 中文 }
  run:
    type: bool
    description: 生成后直接执行提交
---
请根据暂存区的改动（git diff --cached）写一条提交信息，使用{{ "中文" if lang == "zh" else "英文" }}。

{% if style == "conventional" %}
格式遵循 Conventional Commits：`type(scope): subject`，type 从 feat、fix、refactor、docs、test、chore、perf 中选择。
{% endif %}
- 标题不超过 72 个字符，说明做了什么。
- 正文说明为什么这样改，以及需要注意的影响；简单的改动可以省略正文。
- 参考 git log 中已有提交的风格。

{% if run %}
确认无误后直接执行 git commit。
{% else %}
只输出提交信息，不要执行提交。
{% endif %}
{% include "_extra" %}
