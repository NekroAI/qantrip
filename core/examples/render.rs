//! 测试辅助：打印模板渲染结果，供 Spike 测试比对。用法：render <id> <input>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = qantrip_core::render(a[1].clone(), a.get(2).cloned().unwrap_or_else(|| a[1].clone()));
    print!("{}", out.expect("unknown template"));
}
