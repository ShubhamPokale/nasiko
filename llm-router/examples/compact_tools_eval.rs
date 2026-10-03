[package]
name = "nasiko-llm-router"
edition.workspace = true
version.workspace = true

[lib]
path = "src/lib.rs"

[[bin]]
name = "llm-router"
path = "src/bin/llm-router.rs"

[dependencies]
nasiko-secrets.workspace = true
nasiko-pricing.workspace = true
nasiko-savings.workspace = true
nasiko-tool-compact.workspace = true
nasiko-compress.workspace = true

axum = { workspace = true }
tower-http = { workspace = true }
tokio = { workspace = true }
async-trait = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
regex = { workspace = true }
rand = { workspace = true }
rand_distr = { workspace = true }
sqlx = { workspace = true }
reqwest = { workspace = true, features = ["json", "stream"] }
futures = { workspace = true }
async-stream = { workspace = true }
bytes = { workspace = true }
jsonwebtoken = { workspace = true }
dashmap = { workspace = true }
uuid = { workspace = true }
redis = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
serde_json = { workspace = true }
serial_test = { workspace = true }
base64 = { workspace = true }
mockito = { workspace = true }
tower = { workspace = true }
zstd.workspace = true

[[example]]
name = "compact_tools_eval"
path = "examples/compact_tools_eval.rs"
