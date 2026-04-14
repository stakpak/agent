import SwiftUI

struct DependencyOverlay: View {
    @Environment(\.colorScheme) private var colorScheme
    let status: DependencyStatus?
    @Binding var isVisible: Bool
    @State private var showIcon = false
    @State private var showRow1 = false
    @State private var showRow2 = false
    @State private var showRow3 = false
    @State private var dismissing = false

    private var appVersion: String {
        let version = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "—"
        let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "—"
        return "v\(version) (build \(build))"
    }

    private var isTesting: Bool {
        ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil
    }

    var body: some View {
        if isVisible, !isTesting, let status {
            ZStack(alignment: .top) {
                Color(nsColor: .shadowColor).opacity(0.4)
                    .ignoresSafeArea()

                VStack(spacing: 12) {
                    Image("AgentIcon")
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 80, height: 80)
                        .shadow(color: .blue.opacity(0.6), radius: 16)
                        .opacity(showIcon ? 1 : 0)
                        .scaleEffect(showIcon ? 1.0 : 0.8)

                    Text("Agent!")
                        .font(.system(size: 22, weight: .black, design: .monospaced))
                        .opacity(showIcon ? 1 : 0)

                    Text("Agentic AI for your \u{F8FF} Mac Desktop")
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundColor(.secondary)
                        .opacity(showIcon ? 1 : 0)

                    Text(appVersion)
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundColor(.secondary)
                        .opacity(showIcon ? 1 : 0)

                    Text("System Check")
                        .font(.system(.caption, design: .monospaced))
                        .foregroundColor(.secondary)
                        .padding(.top, 4)

                    row(
                        ok: status.xcodeTools,
                        name: "Xcode Command Line Tools",
                        hint: "xcode-select --install",
                        show: showRow1
                    )

                    row(
                        ok: status.clang,
                        name: "Clang Compiler",
                        hint: "Installed with Xcode CLT",
                        show: showRow2
                    )

                    rowWithStatus(
                        ok: status.appleIntelligence,
                        name: "Apple Intelligence",
                        status: status.appleIntelligenceStatus,
                        show: showRow3
                    )

                    if !status.allGood {
                        HStack {
                            Button("Install") {
                                DependencyChecker.installCommandLineTools()
                                dismiss()
                            }
                            .buttonStyle(.borderedProminent)
                            .controlSize(.small)
                            .font(.system(.caption, design: .monospaced))

                            Spacer()

                            Button("Dismiss") {
                                dismiss()
                            }
                            .buttonStyle(.plain)
                            .font(.system(.caption, design: .monospaced))
                            .foregroundColor(.secondary)
                        }
                        .padding(.top, 4)
                    }
                }
                .padding(24)
                .background(
                    RoundedRectangle(cornerRadius: 12)
                        .fill(Color(nsColor: .windowBackgroundColor).opacity(0.95))
                        .shadow(radius: 20)
                )
                .frame(width: 320)
                .padding(.top, 80)
                .scaleEffect(dismissing ? 0.8 : 1.0)
                .opacity(dismissing ? 0 : 1)
            }
            .onAppear {
                SoundPlayer.shared.playStartupSound()
                withAnimation(.easeOut(duration: 0.4)) { showIcon = true }
                withAnimation(.easeOut(duration: 0.3).delay(0.3)) { showRow1 = true }
                withAnimation(.easeOut(duration: 0.3).delay(0.5)) { showRow2 = true }
                withAnimation(.easeOut(duration: 0.3).delay(0.7)) { showRow3 = true }

                // Always auto-dismiss — 2.5s if all good, 5s if something missing
                let delay: Double = status.allGood ? 2.5 : 5.0
                DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
                    dismiss()
                }
            }
        }
    }

    @ViewBuilder
    private func row(ok: Bool, name: String, hint: String, show: Bool) -> some View {
        HStack(spacing: 8) {
            Image(systemName: ok ? "checkmark.circle.fill" : "xmark.circle.fill")
                .foregroundColor(ok ? .green : .red)
                .font(.system(size: 16))
            VStack(alignment: .leading, spacing: 1) {
                Text(name)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundColor(.primary)
                if !ok {
                    Text(hint)
                        .font(.system(size: 9, design: .monospaced))
                        .foregroundColor(.secondary)
                }
            }
            Spacer()
        }
        .opacity(show ? 1 : 0)
        .offset(y: show ? 0 : 8)
    }

    @ViewBuilder
    private func rowWithStatus(ok: Bool, name: String, status: String, show: Bool) -> some View {
        HStack(spacing: 8) {
            Image(systemName: ok ? "checkmark.circle.fill" : "xmark.circle.fill")
                .foregroundColor(ok ? .green : .orange)
                .font(.system(size: 16))
            VStack(alignment: .leading, spacing: 1) {
                Text(name)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundColor(.primary)
                Text(status)
                    .font(.system(size: 9, design: .monospaced))
                    .foregroundColor(ok ? .secondary : .orange)
            }
            Spacer()
        }
        .opacity(show ? 1 : 0)
        .offset(y: show ? 0 : 8)
    }

    private func dismiss() {
        withAnimation(.easeIn(duration: 0.3)) {
            dismissing = true
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            isVisible = false
        }
    }
}
