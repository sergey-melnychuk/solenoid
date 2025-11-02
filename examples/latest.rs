use solenoid::eth::EthClient;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);

    let (block_number, _) = eth.get_latest_block().await?;
    println!("{block_number}");

    Ok(())
}
