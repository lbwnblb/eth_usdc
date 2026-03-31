use std::sync::OnceLock;

static ENV: OnceLock<String> = OnceLock::new();

pub fn get_env() -> &'static str {
    ENV.get_or_init(|| {
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "dev".to_string())
    })
}