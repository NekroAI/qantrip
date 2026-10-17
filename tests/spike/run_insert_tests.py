#!/usr/bin/env python3
"""Spike 插入链路自动化测试（S5 / S6 / 焦点校验）。

前提：dist/ 已构建；Qantrip 已运行、已授予辅助功能权限、开启 debugHooks；Maccy 在运行。
输出每个用例的结果，并汇总 spike.jsonl 中的插入耗时。
"""
import json, os, sqlite3, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DIST = os.path.join(ROOT, "dist")
LOGS = os.path.expanduser("~/Library/Logs/Qantrip")
PROBE = os.path.join(LOGS, "probe.txt")
SPIKE = os.path.join(LOGS, "spike.jsonl")
MACCY = os.path.expanduser(
    "~/Library/Containers/org.p0deje.Maccy/Data/Library/Application Support/Maccy/Storage.sqlite")
PBTOOL = os.path.join(ROOT, "target/spike-helpers/pbtool")

results = []


def sh(*a, **kw):
    return subprocess.run(a, capture_output=True, text=True, **kw).stdout


def debug(*a):
    sh(os.path.join(DIST, "qantrip-debug"), *a)


def notify(name):
    sh("swift", "-e", f'import Foundation; DistributedNotificationCenter.default().postNotificationName(.init("{name}"), object: nil, userInfo: nil, deliverImmediately: true); RunLoop.current.run(until: Date().addingTimeInterval(0.1))')


def frontmost():
    return sh("swift", "-e", "import AppKit; print(NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? \"nil\")").strip()


def pb_dump():
    return sh(PBTOOL, "dump")


def maccy_texts():
    if not os.path.exists(MACCY):
        return []
    con = sqlite3.connect(f"file:{MACCY}?mode=ro", uri=True)
    rows = con.execute("select ZVALUE from ZHISTORYITEMCONTENT where ZTYPE='public.utf8-plain-text'").fetchall()
    con.close()
    return [bytes(r[0]).decode("utf-8", "replace") for r in rows if r[0] is not None]


def check(name, ok, detail=""):
    results.append((name, ok, detail))
    print(("PASS " if ok else "FAIL ") + name + (f"  — {detail}" if detail else ""), flush=True)


def probe_reset():
    notify("ai.nekro.qantrip.probe.reset")
    time.sleep(0.8)


def wait_probe(expected, timeout=3.0):
    end = time.time() + timeout
    got = ""
    while time.time() < end:
        got = open(PROBE, encoding="utf-8").read() if os.path.exists(PROBE) else ""
        if got == expected:
            return got
        time.sleep(0.1)
    return got


def defaults(key, ms):
    if ms is None:
        sh("defaults", "delete", "ai.nekro.qantrip", key)
    else:
        sh("defaults", "write", "ai.nekro.qantrip", key, "-float", str(ms))


TEXTS = {
    "zh": "帮我翻译成中文。",
    "multiline": "请审查当前改动：\n\n1. 先理解意图\n2. 再找问题\n\n不要直接修改代码。",
    "unicode": "中文、日本語、한국어、Ελληνικά、עברית\nEmoji: 🧙‍♂️✨🪄 👩🏽‍💻 🇨🇳\n符号: `code` $HOME %s {{x}} \\n \"quote\" 'single' <tag> & | ;\n全角：，。！？「」——＝",
    "long": "\n".join(f"{i:03} 长文本测试行：The quick brown fox jumps over the lazy dog. 敏捷的棕色狐狸。" for i in range(1, 201)),
}


def case_paste(tid, rich):
    expected = TEXTS[tid]
    if rich:
        sh(PBTOOL, "set-rich", f"rich-sentinel-{tid}")
    else:
        subprocess.run(["pbcopy"], input=f"sentinel-{tid}", text=True)
    before = pb_dump()
    maccy_before = set(maccy_texts())
    probe_reset()
    if frontmost() != "ai.nekro.qantrip.pasteprobe":
        check(f"paste[{tid}] 前置：PasteProbe 在前台", False, frontmost())
        return
    debug("paste", expected)
    got = wait_probe(expected)
    check(f"paste[{tid}] 内容逐字一致（{len(expected)} 字符）", got == expected,
          "" if got == expected else f"收到 {len(got)} 字符")
    time.sleep(0.8)  # 等待恢复（默认 500ms）
    after = pb_dump()
    check(f"paste[{tid}] 剪贴板恢复（{'多格式' if rich else '纯文本'}）", before == after,
          "" if before == after else f"before={before!r} after={after!r}")
    leaked = [t for t in set(maccy_texts()) - maccy_before if expected[:40] in t]
    check(f"paste[{tid}] 未进入 Maccy 历史", not leaked)


def build_helpers():
    os.makedirs(os.path.dirname(PBTOOL), exist_ok=True)
    subprocess.run(["swiftc", "-O", os.path.join(ROOT, "tests/spike/pbtool.swift"), "-o", PBTOOL], check=True)


def main():
    build_helpers()
    # 对照组：普通复制应当被 Maccy 记录，证明 Maccy 正在工作
    marker = f"maccy-control-{int(time.time())}"
    subprocess.run(["pbcopy"], input=marker, text=True)
    time.sleep(1.5)
    check("对照：普通复制被 Maccy 记录", any(marker in t for t in maccy_texts()))

    open(SPIKE, "w").close()
    for tid, rich in [("zh", False), ("multiline", True), ("unicode", False), ("long", False)]:
        case_paste(tid, rich)

    # 面板输入经 Rust 核心渲染后插入
    probe_reset()
    debug("insert", "zh 你好")
    got = wait_probe("帮我翻译成中文。\n\n补充要求：你好")
    check("insert[zh 你好] 经核心渲染后插入", got == "帮我翻译成中文。\n\n补充要求：你好", repr(got[:60]))

    # 展开粘贴（D13）：两次 ⌘V，探针应收到两份相同内容
    probe_reset()
    text = TEXTS["multiline"]
    debug("paste", text, "expand")
    got = wait_probe(text + text, timeout=4)
    check("展开粘贴：连续粘贴两次", got == text + text, f"收到 {len(got)} 字符，期望 {2 * len(text)}")
    time.sleep(0.8)

    # 剪贴板在恢复窗口内被用户修改 → 不覆盖
    defaults("restoreDelayMs", 1500)
    probe_reset()
    subprocess.run(["pbcopy"], input="before-race", text=True)
    debug("paste", TEXTS["zh"])
    time.sleep(0.4)
    subprocess.run(["pbcopy"], input="user-copied-during-window", text=True)
    time.sleep(1.6)
    now = sh("pbpaste")
    check("恢复窗口内用户复制了新内容 → 不被覆盖", now == "user-copied-during-window", repr(now))
    defaults("restoreDelayMs", None)

    # 目标应用在插入前切换 → 降级为只复制，不粘贴到错误应用
    defaults("pasteDelayMs", 800)
    probe_reset()
    # 以当前前台（PasteProbe）为目标触发插入，在发送 ⌘V 前切到 Finder
    debug("paste", TEXTS["zh"])
    time.sleep(0.1)
    sh("open", "-a", "Finder")
    time.sleep(1.3)
    got = open(PROBE, encoding="utf-8").read()
    ev = [json.loads(l) for l in open(SPIKE)]
    last = [e for e in ev if e["event"] == "insert"][-1]
    check("插入前目标应用切换 → 降级只复制", got == "" and last.get("reason") == "target_changed",
          f"probe={got[:20]!r} last={last.get('mode')}/{last.get('reason')}")
    check("降级后 Prompt 留在剪贴板", sh("pbpaste") == TEXTS["zh"])
    defaults("pasteDelayMs", None)

    ev = [json.loads(l) for l in open(SPIKE)]
    pasted = [e for e in ev if e["event"] == "insert" and e["mode"] == "paste"]
    print("\n插入耗时（触发 → 发出 ⌘V）ms:", [e["paste_ms"] for e in pasted])
    print("剪贴板快照耗时 ms:", [e["snapshot_ms"] for e in pasted])

    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)}/{len(results)} 通过")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
