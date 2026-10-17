// 内置模板一览：由 core/templates/*.md 提取（名称、别名、描述、分类、参数名），与客户端内置模板一致。
export interface BuiltinTemplate {
  name: string
  aliases: string[]
  description: string
  category: string
  params: string[]
}

export const BUILTIN_TEMPLATES: BuiltinTemplate[] = [
  {
    "name": "arch",
    "aliases": [],
    "description": "架构评估与技术选型",
    "category": "编程",
    "params": [
      "focus",
      "options"
    ]
  },
  {
    "name": "commit",
    "aliases": [
      "ci"
    ],
    "description": "生成提交信息",
    "category": "编程",
    "params": [
      "style",
      "lang",
      "run"
    ]
  },
  {
    "name": "debug",
    "aliases": [],
    "description": "深度调试：定位并验证问题",
    "category": "编程",
    "params": [
      "mode",
      "log"
    ]
  },
  {
    "name": "docs",
    "aliases": [],
    "description": "文档生成",
    "category": "编程",
    "params": [
      "kind",
      "audience"
    ]
  },
  {
    "name": "en",
    "aliases": [
      "英文"
    ],
    "description": "翻译成英文",
    "category": "日常",
    "params": [
      "style"
    ]
  },
  {
    "name": "explain",
    "aliases": [
      "ex"
    ],
    "description": "代码解释",
    "category": "编程",
    "params": [
      "level",
      "diagram"
    ]
  },
  {
    "name": "fix",
    "aliases": [],
    "description": "问题修复：找根因，最小修改",
    "category": "编程",
    "params": [
      "mode",
      "test"
    ]
  },
  {
    "name": "investigate",
    "aliases": [
      "inv"
    ],
    "description": "问题调查：收集证据、评估假设",
    "category": "编程",
    "params": [
      "depth",
      "hypotheses"
    ]
  },
  {
    "name": "perf",
    "aliases": [],
    "description": "性能优化",
    "category": "编程",
    "params": [
      "focus",
      "measure",
      "mode"
    ]
  },
  {
    "name": "plan",
    "aliases": [],
    "description": "开发计划：拆分需求、规划实施",
    "category": "编程",
    "params": [
      "risks",
      "estimate",
      "steps"
    ]
  },
  {
    "name": "pr",
    "aliases": [],
    "description": "生成 PR 描述",
    "category": "编程",
    "params": [
      "sections",
      "lang",
      "create"
    ]
  },
  {
    "name": "refactor",
    "aliases": [],
    "description": "代码重构：改善结构，行为不变",
    "category": "编程",
    "params": [
      "goal",
      "keep-api",
      "mode"
    ]
  },
  {
    "name": "review",
    "aliases": [
      "r"
    ],
    "description": "代码审查",
    "category": "编程",
    "params": [
      "focus",
      "scope",
      "strict"
    ]
  },
  {
    "name": "security",
    "aliases": [
      "sec"
    ],
    "description": "安全审查",
    "category": "编程",
    "params": [
      "focus",
      "scope"
    ]
  },
  {
    "name": "simplify",
    "aliases": [
      "simp"
    ],
    "description": "代码简化",
    "category": "编程",
    "params": [
      "scope",
      "aggressive",
      "mode"
    ]
  },
  {
    "name": "test",
    "aliases": [],
    "description": "测试生成",
    "category": "编程",
    "params": [
      "kind",
      "framework",
      "mode"
    ]
  },
  {
    "name": "zh",
    "aliases": [
      "中文"
    ],
    "description": "翻译成中文",
    "category": "日常",
    "params": [
      "keep-terms",
      "style"
    ]
  }
]
