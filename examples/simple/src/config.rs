#[app::config(prefix = "DATABASE")]
#[derive(Clone, Debug)]
pub struct DatabaseConfig {
    #[app::property(default = "sqlite::memory:")]
    pub url: String,
}

impl DatabaseConfig {
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".to_owned()),
        }
    }
}
