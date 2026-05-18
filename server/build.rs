fn main() {
    let channel = std::env::var("RELEASE_CHANNEL").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=RELEASE_CHANNEL={}", channel);
    println!("cargo:rerun-if-env-changed=RELEASE_CHANNEL");
}
