use std::time::Instant;

use solenoid::{common::{block::Block, word::Word}, eth, ext::Ext, runner};


#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt::try_init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    let block_number = std::env::args()
        .nth(1)
        .and_then(|number| number.parse::<u64>().ok())
        .unwrap_or(23027350); // https://xkcd.com/221/

    let Block{ header, transactions } = eth.get_full_block(Word::from(block_number)).await?;
    eprintln!("📦 Fetched block number: {} [with {} txs]", header.number.as_usize(), transactions.len());

    let ext = Ext::at_number(Word::from(block_number - 1), eth).await?;

    let mut f = runner(header, ext);

    for tx in transactions {
        let idx = tx.index.as_usize();
        let now = Instant::now();
        match f(tx).await {
            Ok((result, traces)) => {
                let ms = now.elapsed().as_millis();
                eprintln!("TX \tindex={idx} \tOK={} \tGAS={} \tTRACES={} \tms={ms}", 
                    !result.rev, 
                    result.gas,
                    traces.len());
            }
            Err(e) => {
                eprintln!("TX \tindex={idx} \tPANIC: '{e}'"); 
            }
        }
    }

    Ok(())
}