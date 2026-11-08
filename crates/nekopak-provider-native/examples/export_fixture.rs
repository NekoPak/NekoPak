//! Offline configuration fixtures in an isolated namespace; never signs in or sends HTTP.
use nekopak_provider::ProviderPort;
use nekopak_provider_native::NativeProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = NativeProvider::unconfigured("fixture")?;
    let auth = provider.auth_status().await?;
    let mut capabilities = provider.discover().await?;
    capabilities.checked_at = "2026-11-08T00:00:00Z".into();
    let fixture = serde_json::json!({
        "scope": "Offline unconfigured native adapter; not a live account observation.",
        "auth": auth,
        "backend_discover": [capabilities],
    });
    std::fs::write(
        "tests/fixtures/native_unconfigured.json",
        format!("{}\n", serde_json::to_string_pretty(&fixture)?),
    )?;
    let normal = NativeProvider::public_web("native-default-fixture-no-account")?;
    let mut capabilities = normal.discover().await?;
    capabilities.checked_at = "2026-11-08T00:00:00Z".into();
    let fixture = serde_json::json!({
        "scope": "Built-in public Web profile on Windows; no account sign-in or HTTP requests.",
        "auth": normal.auth_status().await?,
        "auth_sign_in_enabled": normal.supports_password_auth(),
        "backend_discover": [capabilities],
    });
    std::fs::write(
        "tests/fixtures/native_default.json",
        format!("{}\n", serde_json::to_string_pretty(&fixture)?),
    )?;
    Ok(())
}
