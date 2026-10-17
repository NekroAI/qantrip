---
name: security
aliases: [sec]
description: 安全审查
category: 审查与测试
params:
  focus:
    type: multi
    positional: true
    description: 审查重点
    choices:
      - { value: auth, label: 认证与权限 }
      - { value: input, label: 输入校验 }
      - { value: injection, label: 注入 }
      - { value: secrets, label: 敏感信息 }
      - { value: deps, label: 依赖 }
  scope:
    type: enum
    description: 审查范围
    default: changes
    choices:
      - { value: changes, label: 所有未提交改动 }
      - { value: staged, label: 暂存区 }
      - { value: branch, label: 当前分支 }
      - { value: last, label: 最近一次提交 }
---
请对{% include "_scope" %}做安全审查{% if focus %}，重点检查{% for f in focus %}{{ {"auth": "认证与权限控制", "input": "输入校验", "injection": "注入（SQL、命令、模板、路径）", "secrets": "密钥和敏感信息泄露", "deps": "依赖的已知漏洞"}[f] }}{% if not loop.last %}、{% endif %}{% endfor %}{% endif %}。

对每个发现：
1. 说明攻击者如何利用（前置条件和影响），不要给出可直接使用的攻击载荷。
2. 评估严重程度（高 / 中 / 低）和置信度。
3. 给出修复建议。

区分"确认可利用"和"需要进一步验证"。不要直接修改代码。
{% include "_extra" %}
