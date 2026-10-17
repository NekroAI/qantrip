---
name: test
description: 测试生成
category: 审查与测试
params:
  kind:
    type: multi
    positional: true
    description: 测试类型
    choices:
      - { value: unit, label: 单元测试 }
      - { value: integration, label: 集成测试 }
      - { value: edge, label: 边界条件 }
      - { value: regression, label: 回归测试 }
  framework:
    type: string
    description: 使用的测试框架（默认沿用项目现有框架）
  mode:
    type: enum
    description: 执行模式
    default: apply
    choices:
      - { value: suggest, label: 只给出测试代码 }
      - { value: apply, label: 直接写入并运行 }
---
请为{% if extra %}{{ extra }}{% else %}当前改动涉及的代码{% endif %}编写{% if kind %}{% for k in kind %}{{ {"unit": "单元测试", "integration": "集成测试", "edge": "边界条件测试", "regression": "回归测试"}[k] }}{% if not loop.last %}、{% endif %}{% endfor %}{% else %}测试{% endif %}。

要求：
1. {% if framework %}使用 {{ framework }}。{% else %}沿用项目现有的测试框架、目录结构和命名习惯。{% endif %}

2. 测试行为而不是实现细节；每个测试只验证一件事，名称能说明预期。
3. 覆盖正常路径、边界值、错误输入和异常分支。
4. 不要为了让测试通过而修改被测代码；发现疑似 bug 时单独指出。
{% if mode == "apply" %}
5. 写完后运行测试并报告结果。
{% endif %}
