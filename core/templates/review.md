---
name: review
aliases: [r]
description: 代码审查
category: 编程
params:
  focus:
    type: multi
    positional: true
    description: 审查重点（可多选）
    choices:
      - { value: correctness, label: 正确性, aliases: [bug] }
      - { value: security, label: 安全, aliases: [sec] }
      - { value: performance, label: 性能, aliases: [perf] }
      - { value: maintainability, label: 可维护性, aliases: [maint] }
      - { value: tests, label: 测试覆盖 }
  scope:
    type: enum
    description: 审查范围
    default: changes
    choices:
      - { value: changes, label: 所有未提交改动 }
      - { value: staged, label: 暂存区 }
      - { value: branch, label: 当前分支 }
      - { value: last, label: 最近一次提交 }
  strict:
    type: bool
    short: s
    description: 严格模式：报告所有疑点
---
请审查{% include "_scope" %}。

先理解改动的意图，再找问题。
{% if focus %}
重点关注：{% for f in focus %}{{ {"correctness": "正确性与边界条件", "security": "安全（权限、输入校验、敏感信息、注入）", "performance": "性能与资源占用", "maintainability": "可读性与可维护性", "tests": "测试是否覆盖了改动的行为"}[f] }}{% if not loop.last %}、{% endif %}{% endfor %}。
{% else %}
关注正确性、边界条件、安全和可维护性。
{% endif %}

输出要求：
1. 按严重程度排序列出问题，每条给出文件与行号、问题原因和修改建议。
2. 区分"确认的问题"和"需要验证的疑点"。
{% if strict %}
3. 严格模式：不确定的地方也列出来并标注置信度，不要因为篇幅省略。
{% else %}
3. 只报告真正值得修改的问题，不要罗列风格偏好。
{% endif %}

不要直接修改代码。
{% include "_extra" %}
