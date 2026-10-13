// The Rust SDK's calls, for checks.sdk-languages.
fn main() {
    rewind_sdk::test_begin("rust demo");
    rewind_sdk::always!(true, "rust-holds");
    for _ in 0..2 {
        rewind_sdk::always!(false, "rust-fails", "{} said so", "rust");
    }
    rewind_sdk::sometimes!(false, "rust-sometimes");
    rewind_sdk::unreachable!("rust-unreachable", "reached");
    rewind_sdk::test_end("rust demo", rewind_sdk::TestResult::Fail);
}
