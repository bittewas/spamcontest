use log::{debug, error, LevelFilter};
use serenity::prelude::*;
use spamcontest::Handler;
use std::{env, io, process};

const TOKEN_VAR_KEY: &str = "DISCORD_TOKEN";

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_module(module_path!(), LevelFilter::Info)
        .parse_default_env()
        .init();

    let token = match env::var(TOKEN_VAR_KEY) {
        Ok(token) => token,
        Err(err) => {
            error!("Unable to get {}: {}", TOKEN_VAR_KEY, err);
            process::exit(1);
        }
    };

    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = match Client::builder(token, intents)
        .event_handler(Handler::new())
        .await
    {
        Ok(client) => client,
        Err(err) => {
            error!("Unable to start client: {}", err);
            process::exit(2);
        }
    };

    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal()
            .await
            .expect("error on waiting for shutdown signal");

        debug!("Shutting down...");
        shard_manager.lock().await.shutdown_all().await;
    });

    if let Err(err) = client.start().await {
        error!("An error occurred while running the client: {}", err);
        process::exit(2);
    }
}

async fn wait_for_shutdown_signal() -> io::Result<()> {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        use tokio::signal::unix;

        let sigterm = async {
            unix::signal(unix::SignalKind::terminate())?.recv().await;
            Ok(())
        };

        tokio::select! {
            result = ctrl_c => { result }
            result = sigterm => { result }
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await
}
