import AppKit
import QantripCore
import SwiftUI

/// 首次启动引导（PRD 11.2 Onboarding）：快捷键确认 → 输入权限（可跳过）→ 施放一次。
enum Onboarding {
    static let hotKeyPressed = Notification.Name("ai.nekro.qantrip.hotKeyPressed")

    static func show() {
        WindowManager.shared.show("onboarding", title: L("欢迎使用 Nekro Qantrip", "Welcome to Nekro Qantrip"), size: NSSize(width: 620, height: 520)) {
            OnboardingView()
        }
    }
}

struct OnboardingView: View {
    @Bindable private var settings = AppSettings.shared
    @State private var step = 0
    @State private var hotKeyVerified = false
    @State private var trusted = Inserter.isTrusted
    @State private var practice = ""
    private let timer = Timer.publish(every: 1, on: .main, in: .common).autoconnect()

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                ForEach(0..<3) { i in
                    Capsule().fill(i <= step ? Color.qAccent : Color.secondary.opacity(0.25)).frame(height: 4)
                }
            }
            .padding(.horizontal, 32).padding(.top, 20)
            Group {
                switch step {
                case 0: hotKeyStep
                case 1: permissionStep
                default: practiceStep
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.horizontal, 40)
            .transition(.asymmetric(insertion: .move(edge: .trailing).combined(with: .opacity), removal: .opacity))
            .id(step)
            footer
        }
        .onReceive(timer) { _ in trusted = Inserter.isTrusted }
        .onReceive(NotificationCenter.default.publisher(for: Onboarding.hotKeyPressed)) { _ in
            withAnimation(.spring(duration: 0.3)) { hotKeyVerified = true }
        }
    }

    private func title(_ t: String, _ sub: String) -> some View {
        VStack(spacing: 8) {
            Text(t).font(.qDisplay(28))
            Text(sub).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
    }

    private var hotKeyStep: some View {
        VStack(spacing: 22) {
            Image(systemName: "wand.and.stars").font(.system(size: 44)).foregroundStyle(Color.qAccent)
                .symbolEffect(.bounce, value: hotKeyVerified)
            title(L("按 Q 施放一个戏法", "Press Q to cast a spell"), L("在任何应用里按下唤起快捷键，输入几个字母，就能把常用 Prompt 插入到当前输入框。", "In any app, press the shortcut, type a few letters, and insert a prompt into the current text field."))
            HStack {
                Text(L("唤起快捷键", "Panel shortcut"))
                HotKeyRecorder()
            }
            Label(hotKeyVerified ? L("快捷键可以正常使用", "The shortcut works") : L("现在按一次 \(settings.hotKey.display) 试试", "Press \(settings.hotKey.display) now to try it"),
                  systemImage: hotKeyVerified ? "checkmark.circle.fill" : "keyboard")
                .foregroundStyle(hotKeyVerified ? .green : .secondary)
                .contentTransition(.symbolEffect(.replace))
        }
    }

    private var permissionStep: some View {
        VStack(spacing: 22) {
            Image(systemName: trusted ? "checkmark.shield.fill" : "hand.raised.fill").font(.system(size: 44))
                .foregroundStyle(trusted ? .green : Color.qAccent)
            title(L("自动粘贴（可选）", "Auto-paste (optional)"), L("授予「辅助功能」权限后，Qantrip 会替你按下 ⌘V，把 Prompt 直接插入当前应用。\n不授权也能用：Prompt 会复制到剪贴板，由你手动粘贴。", "With Accessibility permission, Qantrip presses ⌘V for you and inserts the prompt into the current app.\nWithout it, the prompt is copied to the clipboard for you to paste."))
            if trusted {
                Label(L("已授予辅助功能权限", "Accessibility permission granted"), systemImage: "checkmark.circle.fill").foregroundStyle(.green)
            } else {
                Button(L("授予辅助功能权限…", "Grant Accessibility Permission…")) { Inserter.requestTrust() }.buttonStyle(.borderedProminent)
                Text(L("系统设置 → 隐私与安全性 → 辅助功能，打开 Qantrip 的开关。", "System Settings → Privacy & Security → Accessibility, then turn on Qantrip.")).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var practiceStep: some View {
        VStack(spacing: 18) {
            title(L("施放一次", "Try it once"), L("点一下下面的输入框，按 \(settings.hotKey.display)，输入 zh 后按回车。", "Click the field below, press \(settings.hotKey.display), type zh, then press Return."))
            TextEditor(text: $practice)
                .font(.system(size: 14))
                .frame(height: 120)
                .scrollContentBackground(.hidden)
                .padding(8)
                .background(.background.opacity(0.6), in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.qAccent.opacity(practice.isEmpty ? 0.2 : 0.6)))
            if !practice.isEmpty {
                Label(L("成功了！之后在任何应用里都可以这样使用。", "It worked! You can use it like this in any app."), systemImage: "sparkles").foregroundStyle(Color.qAccent)
                    .transition(.scale.combined(with: .opacity))
            }
            Text(L("小提示：输入 r 后按空格再输入 --，可以看到审查模板的全部参数；面板里也可以用鼠标点选。", "Tip: type r, a space, then -- to see all parameters of the review template. You can also click items in the panel."))
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .animation(.spring(duration: 0.3), value: practice.isEmpty)
    }

    private var footer: some View {
        HStack {
            if step > 0 {
                Button(L("上一步", "Back")) { withAnimation { step -= 1 } }
            }
            Spacer()
            if step == 1 && !trusted {
                Button(L("跳过，先用只复制", "Skip and use copy only")) { withAnimation { step += 1 } }
            }
            if step < 2 {
                Button(L("继续", "Continue")) { withAnimation { step += 1 } }
                    .buttonStyle(.borderedProminent)
                    .disabled(step == 1 && !trusted)
            } else {
                Button(L("开始使用", "Get started")) {
                    settings.onboardingDone = true
                    WindowManager.shared.close("onboarding")
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(20)
    }
}
