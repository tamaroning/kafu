use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

fn init_tracing() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = tracing_subscriber::Registry::default()
        .with(fmt::layer().with_writer(std::io::stderr).with_target(false))
        .with(EnvFilter::from_default_env());
    subscriber.try_init()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing()?;
    kafu_serve::run().await
}
