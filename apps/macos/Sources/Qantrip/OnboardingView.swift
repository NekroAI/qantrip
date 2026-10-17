import AppKit
import QantripCore
import SwiftUI

/// 首次启动引导（PRD 11.2 Onboarding）：快捷键确认 → 输入权限（可跳过）→ 施放一次。
enum Onboarding {
    static let hotKeyPressed = Notification.Name("ai.nekro.qantrip.hotKeyPressed")

    static func show() {
        WindowManager.shared.show("onboarding", title: "欢迎使用 Nekro Qantrip", size: NSSize(width: 620, height: 520)) {
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
            title("按 Q 施放一个戏法", "在任何应用里按下唤起快捷键，输入几个字母，就能把常用 Prompt 插入到当前输入框。")
            HStack {
                Text("唤起快捷键")
                HotKeyRecorder()
            }
            Label(hotKeyVerified ? "快捷键可以正常使用" : "现在按一次 \(settings.hotKey.display) 试试",
                  systemImage: hotKeyVerified ? "checkmark.circle.fill" : "keyboard")
                .foregroundStyle(hotKeyVerified ? .green : .secondary)
                .contentTransition(.symbolEffect(.replace))
        }
    }

    private var permissionStep: some View {
        VStack(spacing: 22) {
            Image(systemName: trusted ? "checkmark.shield.fill" : "hand.raised.fill").font(.system(size: 44))
                .foregroundStyle(trusted ? .green : Color.qAccent)
            title("自动粘贴（可选）", "授予「辅助功能」权限后，Qantrip 会替你按下 ⌘V，把 Prompt 直接插入当前应用。\n不授权也能用：Prompt 会复制到剪贴板，由你手动粘贴。")
            if trusted {
                Label("已授予辅助功能权限", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
            } else {
                Button("授予辅助功能权限…") { Inserter.requestTrust() }.buttonStyle(.borderedProminent)
                Text("系统设置 → 隐私与安全性 → 辅助功能，打开 Qantrip 的开关。").font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var practiceStep: some View {
        VStack(spacing: 18) {
            title("施放一次", "点一下下面的输入框，按 \(settings.hotKey.display)，输入 zh 后按回车。")
            TextEditor(text: $practice)
                .font(.system(size: 14))
                .frame(height: 120)
                .scrollContentBackground(.hidden)
                .padding(8)
                .background(.background.opacity(0.6), in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.qAccent.opacity(practice.isEmpty ? 0.2 : 0.6)))
            if !practice.isEmpty {
                Label("成功了！之后在任何应用里都可以这样使用。", systemImage: "sparkles").foregroundStyle(Color.qAccent)
                    .transition(.scale.combined(with: .opacity))
            }
            Text("小提示：输入 r 后按空格再输入 --，可以看到审查模板的全部参数；面板里也可以用鼠标点选。")
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .animation(.spring(duration: 0.3), value: practice.isEmpty)
    }

    private var footer: some View {
        HStack {
            if step > 0 {
                Button("上一步") { withAnimation { step -= 1 } }
            }
            Spacer()
            if step == 1 && !trusted {
                Button("跳过，先用只复制") { withAnimation { step += 1 } }
            }
            if step < 2 {
                Button("继续") { withAnimation { step += 1 } }
                    .buttonStyle(.borderedProminent)
                    .disabled(step == 1 && !trusted)
            } else {
                Button("开始使用") {
                    settings.onboardingDone = true
                    WindowManager.shared.close("onboarding")
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(20)
    }
}
