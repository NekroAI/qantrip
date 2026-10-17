---
name: debug
description: 深度调试：定位并验证问题
category: 编写与修复
params:
  mode:
    type: enum
    description: 执行模式
    default: read
    choices:
      - { value: read, label: 只定位 }
      - { value: suggest, label: 定位并给方案 }
      - { value: apply, label: 定位并修复 }
  log:
    type: bool
    short: l
    description: 允许添加临时日志辅助定位
---
帮我调试这个问题。
{% if extra %}

现象：{{ extra }}
{% endif %}

按以下步骤进行：
1. 复述现象，列出已知事实，区分事实和推测。
2. 提出最多 3 个可能的原因，按可能性排序，并说明每个原因的验证方法。
3. 逐个验证，每一步都基于证据推进，不要猜测着修改代码。
{% if log %}
4. 可以添加临时日志或调试输出来收集证据，结束后全部移除。
{% endif %}

最后给出根因结论和证据。
{% include "_mode" %}
