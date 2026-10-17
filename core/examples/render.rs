//! 调试辅助：打印输入的解析结果与最终文本。用法：render "<面板输入>"
fn main() {
    let input = std::env::args().nth(1).unwrap_or_default();
    let engine = qantrip_core::Engine::new(None);
    let a = engine.analyze(input.clone(), None);
    eprintln!("enter={:?} canonical={:?} hint={:?}", a.enter, a.canonical, a.hint);
    for d in &a.diagnostics {
        eprintln!("{:?}: {}", d.severity, d.message);
    }
    if let Some(text) = engine.render(input, None) {
        print!("{text}");
    }
}
