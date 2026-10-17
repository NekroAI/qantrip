---
name: test
description: Write tests
category: Review & test
params:
  kind:
    type: multi
    positional: true
    description: Kinds of tests
    choices:
      - { value: unit, label: Unit tests }
      - { value: integration, label: Integration tests }
      - { value: edge, label: Edge cases }
      - { value: regression, label: Regression tests }
  framework:
    type: string
    description: Test framework (defaults to the one the project uses)
  mode:
    type: enum
    description: Execution mode
    default: apply
    choices:
      - { value: suggest, label: Show the tests only }
      - { value: apply, label: Write and run them }
---
Write {% if kind %}{% for k in kind %}{{ {"unit": "unit tests", "integration": "integration tests", "edge": "edge-case tests", "regression": "regression tests"}[k] }}{% if not loop.last %} and {% endif %}{% endfor %}{% else %}tests{% endif %} for {% if extra %}{{ extra }}{% else %}the code touched by the current changes{% endif %}.

Requirements:
1. {% if framework %}Use {{ framework }}.{% else %}Follow the project's existing test framework, layout and naming.{% endif %}

2. Test behavior, not implementation details; one thing per test, with a name that states the expectation.
3. Cover the normal path, boundary values, invalid input and error branches.
4. Don't change the code under test to make tests pass; point out suspected bugs separately.
{% if mode == "apply" %}
5. Run the tests when you're done and report the results.
{% endif %}
