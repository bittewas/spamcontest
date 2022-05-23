use serenity::prelude::*;
use spamcontest::Handler;
use std::{env, process};

const TOKEN_VAR_KEY: &str = "DISCORD_TOKEN";

#[tokio::main]
async fn main() {
    let token = match env::var(TOKEN_VAR_KEY) {
        Ok(token) => token,
        Err(err) => {
            eprintln!("Unable to get {}: {}", TOKEN_VAR_KEY, err);
            process::exit(1)
        }
    };

    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = match Client::builder(token, intents)
        .event_handler(Handler::new())
        .await
    {
        Ok(client) => client,
        Err(err) => {
            eprintln!("Unable to start client: {}", err);
            process::exit(2)
        }
    };

    if let Err(err) = client.start().await {
        eprintln!("An Error occurred while running the client: {:?}", err)
    }
}
