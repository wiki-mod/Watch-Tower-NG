fn main() {
    // Set WATCHTOWER_VERSION at compile time.
    // If not provided via environment, default to "v0.0.0-unknown".
    let version = std::env::var("WATCHTOWER_VERSION")
        .unwrap_or_else(|_| "v0.0.0-unknown".to_string());
    println!("cargo:rustc-env=WATCHTOWER_VERSION={}", version);
}
