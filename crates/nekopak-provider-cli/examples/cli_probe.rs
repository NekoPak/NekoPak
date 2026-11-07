use nekopak_provider::{ProviderPort, RequestContext};
use nekopak_provider_cli::OfficialCliProvider;
use std::path::Path;

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        eprintln!(
            "Usage: cli_probe <absolute pikpak.exe path> <absolute existing profile directory>"
        );
        std::process::exit(2);
    }
    let result = async {
        let provider =
            OfficialCliProvider::configured(Path::new(&args[0]), Path::new(&args[1]), true)?;
        let context = RequestContext::default();
        let probe = context.run(provider.probe()).await?;
        let auth = context.run(provider.auth_status()).await?;
        let capabilities = context.run(provider.discover()).await?;
        Ok::<_, nekopak_domain::ClientError>(
            serde_json::json!({"probe":probe,"auth":auth,"capabilities":capabilities}),
        )
    }
    .await;
    match result {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => {
            eprintln!("{}", serde_json::to_string(&error).unwrap());
            std::process::exit(1);
        }
    }
}
