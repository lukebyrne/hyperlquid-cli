use std::{str::FromStr, sync::Arc, time::Instant};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use ethers::{
    signers::{LocalWallet, Signer},
    types::Address,
};
use hyperliquid::{
    Exchange, Hyperliquid,
    types::{
        Chain,
        exchange::{
            request::{CancelRequest, Limit, OrderRequest, OrderType},
            response::{Response as ExchangeResponse, Status, StatusType},
        },
    },
};

use hyperliquid_cli::{
    asset_index, config,
    db::{self, CreateAccountInput},
    hl_api::{HlApi, UserRoleResponse},
    order_config,
    output::{self, OutputOptions},
    paths, prompt,
    server::client::ServerClient,
    validation, watch, ws,
};

#[derive(Parser, Debug)]
#[command(name = "rhl", about = "CLI for Hyperliquid DEX", version)]
struct Cli {
    /// Output in JSON format
    #[arg(long, global = true, default_value_t = false)]
    json: bool,

    /// Use testnet instead of mainnet
    #[arg(long, global = true, default_value_t = false)]
    testnet: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    Markets {
        #[command(subcommand)]
        command: MarketsCommand,
    },
    Asset {
        #[command(subcommand)]
        command: AssetCommand,
    },
    Order {
        #[command(subcommand)]
        command: OrderCommand,
    },
    #[command(hide = true)]
    Referral {
        #[command(subcommand)]
        command: ReferralCommand,
    },
    Server {
        #[command(subcommand)]
        command: ServerCommand,
    },
    Upgrade,
}

#[derive(Subcommand, Debug)]
enum AccountCommand {
    Add,
    Ls,
    SetDefault(AccountSetDefaultArgs),
    Remove(AccountRemoveArgs),
    Positions(AccountUserWatchArgs),
    Orders(AccountUserWatchArgs),
    Balances(AccountUserWatchArgs),
    Portfolio(AccountUserWatchArgs),
}

#[derive(Args, Debug)]
struct AccountSetDefaultArgs {
    /// Account alias to set as default
    alias: String,
}

#[derive(Args, Debug)]
struct AccountRemoveArgs {
    /// Account alias to remove
    alias: String,

    /// Skip confirmation prompt
    #[arg(short = 'f', long, default_value_t = false)]
    force: bool,
}

#[derive(Args, Debug, Clone)]
struct AccountUserWatchArgs {
    /// User address (defaults to configured wallet)
    #[arg(long)]
    user: Option<String>,

    /// Watch mode - stream real-time updates
    #[arg(short, long, default_value_t = false)]
    watch: bool,
}

#[derive(Subcommand, Debug)]
enum MarketsCommand {
    Ls(MarketsLsArgs),
    #[command(hide = true)]
    Prices(MarketsPricesArgs),
}

#[derive(Args, Debug, Clone)]
struct MarketsLsArgs {
    /// List only spot markets
    #[arg(long, default_value_t = false)]
    spot_only: bool,

    /// List only perpetual markets
    #[arg(long, default_value_t = false)]
    perp_only: bool,

    /// Watch mode - stream real-time updates
    #[arg(short, long, default_value_t = false)]
    watch: bool,
}

#[derive(Args, Debug, Clone)]
struct MarketsPricesArgs {
    /// Watch mode - stream real-time updates
    #[arg(short, long, default_value_t = false)]
    watch: bool,
}

#[derive(Subcommand, Debug)]
enum AssetCommand {
    Price(AssetPriceArgs),
    Book(AssetBookArgs),
    Leverage(AssetLeverageArgs),
}

#[derive(Args, Debug, Clone)]
struct AssetPriceArgs {
    /// Coin symbol (e.g., BTC, ETH)
    coin: String,

    /// Watch mode - stream real-time updates
    #[arg(short, long, default_value_t = false)]
    watch: bool,
}

#[derive(Args, Debug, Clone)]
struct AssetBookArgs {
    /// Coin symbol (e.g., BTC, ETH)
    coin: String,

    /// Watch mode - stream real-time updates
    #[arg(short, long, default_value_t = false)]
    watch: bool,
}

#[derive(Args, Debug, Clone)]
struct AssetLeverageArgs {
    /// Coin symbol (e.g., BTC, ETH)
    coin: String,

    /// User address (defaults to configured wallet)
    #[arg(long)]
    user: Option<String>,

    /// Watch mode - stream real-time updates
    #[arg(short, long, default_value_t = false)]
    watch: bool,
}

#[derive(Subcommand, Debug)]
enum OrderCommand {
    Ls(AccountUserWatchArgs),
    Limit(OrderLimitArgs),
    Market(OrderMarketArgs),
    Cancel(OrderCancelArgs),
    CancelAll(OrderCancelAllArgs),
    SetLeverage(OrderSetLeverageArgs),
    Configure(OrderConfigureArgs),
}

#[derive(Args, Debug, Clone)]
struct OrderLimitArgs {
    /// Order side: buy, sell, long, or short
    side: String,
    /// Order size
    size: String,
    /// Coin symbol (e.g., BTC, ETH)
    coin: String,
    /// Limit price
    price: String,

    /// Time-in-force: Gtc, Ioc, Alo
    #[arg(long, default_value = "Gtc")]
    tif: String,

    /// Reduce-only order
    #[arg(long, default_value_t = false)]
    reduce_only: bool,
}

#[derive(Args, Debug, Clone)]
struct OrderMarketArgs {
    /// Order side: buy, sell, long, or short
    side: String,
    /// Order size
    size: String,
    /// Coin symbol (e.g., BTC, ETH)
    coin: String,

    /// Reduce-only order
    #[arg(long, default_value_t = false)]
    reduce_only: bool,

    /// Slippage percentage (overrides config)
    #[arg(long)]
    slippage: Option<String>,
}

#[derive(Args, Debug, Clone)]
struct OrderCancelArgs {
    /// Order ID to cancel (interactive if omitted)
    oid: Option<String>,
}

#[derive(Args, Debug, Clone)]
struct OrderCancelAllArgs {
    /// Skip confirmation prompt
    #[arg(short = 'y', long, default_value_t = false)]
    yes: bool,

    /// Only cancel orders for a specific coin
    #[arg(long)]
    coin: Option<String>,
}

#[derive(Args, Debug, Clone)]
struct OrderSetLeverageArgs {
    /// Coin symbol
    coin: String,
    /// Leverage value
    leverage: String,

    /// Use cross margin
    #[arg(long, default_value_t = false)]
    cross: bool,

    /// Use isolated margin
    #[arg(long, default_value_t = false)]
    isolated: bool,
}

#[derive(Args, Debug, Clone)]
struct OrderConfigureArgs {
    /// Set default slippage percentage for market orders
    #[arg(long)]
    slippage: Option<String>,
}

#[derive(Subcommand, Debug)]
enum ReferralCommand {
    Set(ReferralSetArgs),
    Status,
}

#[derive(Args, Debug, Clone)]
struct ReferralSetArgs {
    /// Referral code
    code: String,
}

#[derive(Subcommand, Debug)]
enum ServerCommand {
    Start,
    Stop,
    Status,
}

#[tokio::main]
async fn main() {
    install_broken_pipe_panic_hook();

    let cli = Cli::parse();
    let start = Instant::now();

    let output_opts = OutputOptions { json: cli.json };
    let result = run(cli, output_opts).await;

    match result {
        Ok(()) => {
            if !output_opts.json {
                let duration = start.elapsed().as_secs_f64();
                println!("\nCompleted in {:.2}s", duration);
            }
        }
        Err(err) => {
            output::print_error(err.to_string());
            std::process::exit(1);
        }
    }
}

fn install_broken_pipe_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied());

        if let Some(msg) = message
            && msg.contains("Broken pipe")
        {
            std::process::exit(0);
        }

        default_hook(info);
    }));
}

async fn run(cli: Cli, output_opts: OutputOptions) -> Result<()> {
    let cfg = config::load_config(cli.testnet)?;
    let api = HlApi::new(cli.testnet)?;

    match cli.command {
        Command::Account { command } => run_account(command, &cfg, &api, output_opts).await,
        Command::Markets { command } => run_markets(command, &api, output_opts).await,
        Command::Asset { command } => run_asset(command, &cfg, &api, output_opts).await,
        Command::Order { command } => run_order(command, &cfg, &api, output_opts).await,
        Command::Referral { command } => run_referral(command, &cfg, &api, output_opts).await,
        Command::Server { command } => run_server(command, cli.testnet, output_opts).await,
        Command::Upgrade => run_upgrade(output_opts).await,
    }
}

fn require_wallet_address(cfg: &config::LoadedConfig) -> Result<Address> {
    cfg.wallet_address.ok_or_else(|| {
        anyhow!("No account configured. Run 'rhl account add' to set up your account.")
    })
}

fn resolve_user_address(cfg: &config::LoadedConfig, user: &Option<String>) -> Result<Address> {
    if let Some(u) = user {
        return validation::validate_address(u);
    }
    require_wallet_address(cfg)
}

fn exchange_chain(testnet: bool) -> Chain {
    if testnet {
        Chain::ArbitrumTestnet
    } else {
        Chain::Arbitrum
    }
}

fn require_wallet_signer(cfg: &config::LoadedConfig) -> Result<Arc<LocalWallet>> {
    let Some(pk) = &cfg.private_key else {
        if let Some(acc) = &cfg.account
            && acc.account_type == config::AccountType::Readonly
        {
            return Err(anyhow!(
                "Account \"{}\" is read-only and cannot perform trading operations.\nRun 'rhl account add' to set up an API wallet for trading.",
                acc.alias
            ));
        }
        return Err(anyhow!(
            "No account configured. Run 'rhl account add' to set up your account."
        ));
    };
    let pk = validation::validate_private_key(pk)?;
    let wallet = LocalWallet::from_str(&pk).context("Invalid private key")?;
    Ok(Arc::new(wallet))
}

fn print_exchange_response(resp: &ExchangeResponse) {
    match resp {
        ExchangeResponse::Ok(data) => {
            if let Some(status_type) = &data.data {
                match status_type {
                    StatusType::Statuses(statuses) => {
                        for s in statuses {
                            print_exchange_status(s);
                        }
                    }
                    StatusType::Status(status) => print_exchange_status(status),
                    StatusType::Address(addr) => {
                        output::print_success(format!("{addr:#x}"));
                    }
                }
            } else {
                output::print_success(format!("Ok ({})", data.type_));
            }
        }
        ExchangeResponse::Err(err) => output::print_error(err),
    }
}

fn print_exchange_status(status: &Status) {
    match status {
        Status::Resting(r) => output::print_success(format!("Order placed: ID {}", r.oid)),
        Status::Filled(f) => {
            output::print_success(format!("Order filled: {} @ {}", f.total_sz, f.avg_px))
        }
        Status::Error(e) => output::print_error(format!("Order error: {e}")),
        Status::Success => output::print_success("Order status: success"),
        Status::WaitingForFill => output::print_success("Order status: waitingForFill"),
        Status::WaitingForTrigger => output::print_success("Order status: waitingForTrigger"),
        Status::Running(t) => {
            output::print_success(format!("Order status: running (twapId: {})", t.twap_id))
        }
    }
}

async fn run_account(
    cmd: AccountCommand,
    cfg: &config::LoadedConfig,
    api: &HlApi,
    output_opts: OutputOptions,
) -> Result<()> {
    match cmd {
        AccountCommand::Add => account_add(cfg, api, output_opts).await,
        AccountCommand::Ls => account_ls(output_opts),
        AccountCommand::SetDefault(args) => account_set_default(args, output_opts).await,
        AccountCommand::Remove(args) => account_remove(args, output_opts).await,
        AccountCommand::Positions(args) => account_positions(cfg, api, args, output_opts).await,
        AccountCommand::Orders(args) => account_orders(cfg, api, args, output_opts).await,
        AccountCommand::Balances(args) => account_balances(cfg, api, args, output_opts).await,
        AccountCommand::Portfolio(args) => account_portfolio(cfg, api, args, output_opts).await,
    }
}

async fn account_add(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    output_opts: OutputOptions,
) -> Result<()> {
    println!("\n=== Add New Account ===\n");

    let setup_method = prompt::select(
        "How would you like to add your account?",
        vec![
            prompt::SelectOption {
                value: "existing",
                label: "Use existing wallet".to_string(),
            },
            prompt::SelectOption {
                value: "new",
                label: "Create new wallet".to_string(),
            },
            prompt::SelectOption {
                value: "readonly",
                label: "Add read-only account".to_string(),
            },
        ],
    )?;

    match setup_method {
        "new" => {
            println!("\nCreating new wallets with encrypted keystores is coming soon!");
            println!("For now, please use an existing wallet or add a read-only account.\n");
            Ok(())
        }
        "existing" => account_add_existing_wallet(cfg, api, output_opts).await,
        "readonly" => account_add_readonly(output_opts).await,
        _ => Err(anyhow!("Unknown setup method")),
    }
}

async fn account_add_existing_wallet(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    output_opts: OutputOptions,
) -> Result<()> {
    const REFERRAL_LINK: &str = "https://app.hyperliquid.xyz/join/CHRISLING";

    let api_url = if cfg.testnet {
        "https://app.hyperliquid-testnet.xyz/API"
    } else {
        "https://app.hyperliquid.xyz/API"
    };
    println!("\nVisit {api_url} and click \"Generate\" to create one.\n");

    let api_private_key = loop {
        let input = prompt::prompt("Enter your API wallet private key: ")?;
        match validation::validate_private_key(&input) {
            Ok(pk) => break pk,
            Err(e) => {
                output::print_error(e.to_string());
            }
        }
    };

    // Validate API key by checking userRole(address) is agent
    println!("\nValidating API key...");
    let wallet = LocalWallet::from_str(&api_private_key).context("Invalid private key")?;
    let api_wallet_address = wallet.address();

    let role = api.user_role(api_wallet_address).await?;
    let master_address = match role {
        UserRoleResponse::Agent { data } => data.user,
        UserRoleResponse::Missing => {
            return Err(anyhow!(
                "This key is not registered as an API wallet on Hyperliquid"
            ));
        }
        other => {
            return Err(anyhow!(
                "Invalid role: {:?}. Expected an agent wallet.",
                other
            ));
        }
    };

    println!(
        "Valid API wallet for {}",
        output::format_short_address(&master_address)
    );

    println!(
        "\n{}{}{}",
        output::style_warning("🎁 Get "),
        output::style_profit_bold("4% off"),
        output::style_warning(" trading fees with our referral link:")
    );
    println!("   {}", output::style_link(REFERRAL_LINK));
    println!();

    let open_referral =
        prompt::press_enter_or_esc("Press Enter to open in browser, or Esc to skip")?;
    if open_referral {
        let _ = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
                .arg(REFERRAL_LINK)
                .status()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("cmd")
                .args(["/C", "start", REFERRAL_LINK])
                .status()
        } else {
            std::process::Command::new("xdg-open")
                .arg(REFERRAL_LINK)
                .status()
        };
    }

    let alias = prompt_for_alias()?;

    let mut set_as_default = false;
    let existing_count = db::get_account_count()?;
    if existing_count > 0 {
        set_as_default = prompt::confirm("\nSet this as your default account?", true)?;
    }

    let new_account = db::create_account(CreateAccountInput {
        alias: alias.clone(),
        user_address: master_address.clone(),
        account_type: "api_wallet".to_string(),
        source: Some("cli_import".to_string()),
        api_wallet_private_key: Some(api_private_key.clone()),
        api_wallet_public_key: Some(format!("{api_wallet_address:#x}")),
        set_as_default,
    })?;

    if output_opts.json {
        let mut redacted = new_account.clone();
        redacted.api_wallet_private_key = Some("[REDACTED]".to_string());
        output::print_json_pretty(&redacted)?;
        return Ok(());
    }

    println!();
    output::print_success(format!("Account \"{alias}\" added successfully!"));
    println!("\nAccount details:");
    println!("  Alias: {}", new_account.alias);
    println!("  Address: {}", new_account.user_address);
    println!("  Type: {}", new_account.account_type);
    if let Some(api_wallet) = &new_account.api_wallet_public_key {
        println!("  API Wallet: {}", api_wallet);
    }
    println!(
        "  Default: {}",
        if new_account.is_default { "Yes" } else { "No" }
    );
    println!();

    Ok(())
}

async fn account_add_readonly(output_opts: OutputOptions) -> Result<()> {
    println!();
    let user_address_input = prompt::prompt("Enter the wallet address to watch: ")?;
    let user_address = validation::validate_address(&user_address_input)?;
    let alias = prompt_for_alias()?;

    let mut set_as_default = false;
    let existing_count = db::get_account_count()?;
    if existing_count > 0 {
        set_as_default = prompt::confirm("\nSet this as your default account?", true)?;
    }

    let new_account = db::create_account(CreateAccountInput {
        alias: alias.clone(),
        user_address: format!("{user_address:#x}"),
        account_type: "readonly".to_string(),
        source: Some("cli_import".to_string()),
        api_wallet_private_key: None,
        api_wallet_public_key: None,
        set_as_default,
    })?;

    if output_opts.json {
        output::print_json_pretty(&new_account)?;
        return Ok(());
    }

    println!();
    output::print_success(format!("Account \"{alias}\" added successfully!"));
    println!("\nAccount details:");
    println!("  Alias: {}", new_account.alias);
    println!("  Address: {}", new_account.user_address);
    println!("  Type: {}", new_account.account_type);
    println!(
        "  Default: {}",
        if new_account.is_default { "Yes" } else { "No" }
    );
    println!();
    Ok(())
}

fn prompt_for_alias() -> Result<String> {
    loop {
        let alias = prompt::prompt("Enter an alias for this account (e.g., 'main', 'trading'): ")?;
        if alias.trim().is_empty() {
            println!("Alias cannot be empty.");
            continue;
        }
        if db::is_alias_taken(&alias)? {
            println!("Alias \"{alias}\" is already taken. Please choose another.");
            continue;
        }
        return Ok(alias);
    }
}

fn account_ls(output_opts: OutputOptions) -> Result<()> {
    let accounts = db::get_all_accounts()?;

    if output_opts.json {
        let json_accounts: Vec<_> = accounts
            .iter()
            .map(|acc| {
                let mut a = acc.clone();
                a.api_wallet_private_key = None;
                a
            })
            .collect();
        output::print_json_pretty(&json_accounts)?;
        return Ok(());
    }

    if accounts.is_empty() {
        println!("{}", output::style_muted("No accounts found."));
        println!(
            "{}",
            output::style_muted("Run 'rhl account add' to add your first account.")
        );
        return Ok(());
    }

    let rows: Vec<Vec<String>> = accounts
        .iter()
        .map(|acc| {
            vec![
                if acc.is_default {
                    "*".to_string()
                } else {
                    "".to_string()
                },
                acc.alias.clone(),
                output::format_short_address(&acc.user_address),
                acc.account_type.clone(),
                acc.api_wallet_public_key
                    .as_ref()
                    .map(|s| output::format_short_address(s))
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();

    let columns = [
        output::TableColumn::left_with_width("", 2),
        output::TableColumn::left("Alias"),
        output::TableColumn::left("Address"),
        output::TableColumn::left("Type"),
        output::TableColumn::left("API Wallet"),
    ];
    output::print_table_with_columns(&columns, rows);
    println!();
    println!("{}", output::style_muted("* = default account"));
    Ok(())
}

async fn account_set_default(
    args: AccountSetDefaultArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let alias = args.alias;

    let existing = db::get_account_by_alias(&alias)?;
    let Some(existing) = existing else {
        return Err(anyhow!(
            "Account with alias \"{alias}\" not found. Run 'rhl account ls' to see available accounts."
        ));
    };

    if existing.is_default {
        if output_opts.json {
            output::print_json_pretty(&serde_json::json!({
                "message": format!("Account \"{alias}\" is already the default")
            }))?;
        } else {
            println!("Account \"{alias}\" is already the default.");
        }
        return Ok(());
    }

    let updated = db::set_default_account(&alias)?;

    if output_opts.json {
        output::print_json_pretty(&serde_json::json!({
            "id": updated.id,
            "alias": updated.alias,
            "userAddress": updated.user_address,
            "type": updated.account_type,
            "isDefault": updated.is_default,
        }))?;
        return Ok(());
    }

    output::print_success(format!("Account \"{alias}\" is now the default."));
    Ok(())
}

async fn account_remove(args: AccountRemoveArgs, output_opts: OutputOptions) -> Result<()> {
    let existing = db::get_account_by_alias(&args.alias)?;
    let Some(existing) = existing else {
        return Err(anyhow!(
            "Account with alias \"{}\" not found. Run 'rhl account ls' to see available accounts.",
            args.alias
        ));
    };

    if !args.force {
        let confirmed = prompt::confirm(
            &format!(
                "Are you sure you want to remove account \"{}\" ({})?",
                args.alias, existing.user_address
            ),
            false,
        )?;
        if !confirmed {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let deleted = db::delete_account(&args.alias)?;
    if !deleted {
        return Err(anyhow!("Failed to remove account \"{}\"", args.alias));
    }

    if output_opts.json {
        output::print_json_pretty(&serde_json::json!({
            "message": format!("Account \"{}\" removed", args.alias),
            "deleted": true
        }))?;
    } else {
        output::print_success(format!("Account \"{}\" removed.", args.alias));
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct PositionRow {
    coin: String,
    size: String,
    #[serde(rename = "entryPx")]
    entry_px: Option<String>,
    #[serde(rename = "positionValue")]
    position_value: String,
    #[serde(rename = "unrealizedPnl")]
    unrealized_pnl: String,
    leverage: String,
    #[serde(rename = "liquidationPx")]
    liquidation_px: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AllDexsClearinghouseStateEvent {
    clearinghouse_states: Vec<(String, hyperliquid_cli::hl_api::ClearinghouseState)>,
}

fn format_pnl_cell(value: &str) -> String {
    let Ok(num_value) = value.parse::<f64>() else {
        return output::style_muted("-");
    };
    if !num_value.is_finite() {
        return output::style_muted("-");
    }

    let sign = if num_value > 0.0 { "+" } else { "" };
    let formatted = format!("{sign}{num_value:.2}");
    if num_value > 0.0 {
        output::style_profit(formatted)
    } else if num_value < 0.0 {
        output::style_loss(formatted)
    } else {
        formatted
    }
}

async fn account_positions(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: AccountUserWatchArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let user = resolve_user_address(cfg, &args.user)?;
    let table_columns = [
        output::TableColumn::left("Coin"),
        output::TableColumn::right("Size"),
        output::TableColumn::right("Entry"),
        output::TableColumn::right("Value"),
        output::TableColumn::right("PnL"),
        output::TableColumn::right("Leverage"),
        output::TableColumn::right("Liq. Price"),
    ];

    if args.watch {
        if !output_opts.json {
            watch::hide_cursor();
        }

        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let mut last_updated = watch::format_timestamp();
        let mut positions: Vec<PositionRow> = Vec::new();
        let mut account_value = "0".to_string();
        let mut total_margin_used = "0".to_string();

        let render = |positions: &[PositionRow],
                      account_value: &str,
                      total_margin_used: &str,
                      last_updated: &str| {
            watch::clear_screen();
            println!("{}", output::format_watch_header("Positions", last_updated));
            println!();

            if positions.is_empty() {
                println!("{}", output::style_muted("No open positions"));
            } else {
                let rows: Vec<Vec<String>> = positions
                    .iter()
                    .map(|p| {
                        vec![
                            p.coin.clone(),
                            p.size.clone(),
                            p.entry_px.clone().unwrap_or_else(|| "-".to_string()),
                            p.position_value.clone(),
                            format_pnl_cell(&p.unrealized_pnl),
                            p.leverage.clone(),
                            p.liquidation_px.clone(),
                        ]
                    })
                    .collect();
                output::print_table_with_columns(&table_columns, rows);
            }

            println!();
            println!("Account Value: {}", output::style_bold(account_value));
            println!(
                "Total Margin Used: {}",
                output::style_bold(total_margin_used)
            );
            println!();
            println!("{}", output::style_muted("Press Ctrl+C to exit"));
        };

        if !output_opts.json {
            render(
                &positions,
                &account_value,
                &total_margin_used,
                &last_updated,
            );
        }

        'outer: loop {
            let mut client = match ws::WsClient::connect(api.testnet).await {
                Ok(c) => c,
                Err(e) => {
                    if !output_opts.json {
                        watch::clear_screen();
                        println!("{}", output::style_loss(format!("Error: {e}")));
                        println!("{}", output::style_muted("Reconnecting..."));
                    } else {
                        output::print_error(e.to_string());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
            };

            if let Err(e) = client
                .subscribe(ws::sub_all_dexs_clearinghouse_state(user))
                .await
            {
                if !output_opts.json {
                    watch::clear_screen();
                    println!("{}", output::style_loss(format!("Error: {e}")));
                    println!("{}", output::style_muted("Reconnecting..."));
                } else {
                    output::print_error(e.to_string());
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }

            loop {
                tokio::select! {
                    msg = client.next_json() => {
                        let msg = match msg {
                            Ok(Some(m)) => m,
                            Ok(None) => break,
                            Err(e) => {
                                if !output_opts.json {
                                    watch::clear_screen();
                                    println!("{}", output::style_loss(format!("Error: {e}")));
                                    println!("{}", output::style_muted("Reconnecting..."));
                                } else {
                                    output::print_error(e.to_string());
                                }
                                break;
                            }
                        };

                        let Ok(channel) = ws::WsClient::channel(&msg) else {
                            continue;
                        };
                        if channel != "allDexsClearinghouseState" {
                            continue;
                        }

                        let Ok(data) = ws::WsClient::data(&msg) else {
                            continue;
                        };
                        let Ok(event) = serde_json::from_value::<AllDexsClearinghouseStateEvent>(data.clone()) else {
                            continue;
                        };

                        let clearinghouse_states = event.clearinghouse_states;
                        if let Some((_, main)) = clearinghouse_states.first() {
                            account_value = main.margin_summary.account_value.clone();
                            total_margin_used = main.margin_summary.total_margin_used.clone();
                        } else {
                            account_value = "0".to_string();
                            total_margin_used = "0".to_string();
                        }

                        positions = clearinghouse_states
                            .into_iter()
                            .flat_map(|(_, st)| st.asset_positions.into_iter().map(|ap| ap.position))
                            .filter(|p| p.szi.parse::<f64>().unwrap_or(0.0) != 0.0)
                            .map(|p| PositionRow {
                                coin: p.coin,
                                size: p.szi,
                                entry_px: p.entry_px,
                                position_value: p.position_value,
                                unrealized_pnl: p.unrealized_pnl,
                                leverage: format!("{}x {}", p.leverage.value, p.leverage.leverage_type),
                                liquidation_px: p.liquidation_px.unwrap_or_else(|| "-".to_string()),
                            })
                            .collect();

                        last_updated = watch::format_timestamp();

                        if output_opts.json {
                            output::print_json_line(&serde_json::json!({
                                "positions": positions,
                                "accountValue": account_value,
                                "totalMarginUsed": total_margin_used,
                                "timestamp": chrono::Utc::now().to_rfc3339(),
                            }))?;
                        } else {
                            render(&positions, &account_value, &total_margin_used, &last_updated);
                        }
                    }
                    _ = &mut ctrl_c => break 'outer,
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let state = api.clearinghouse_state(user).await?;
    let positions: Vec<PositionRow> = state
        .asset_positions
        .iter()
        .map(|p| &p.position)
        .filter(|p| p.szi.parse::<f64>().unwrap_or(0.0) != 0.0)
        .map(|p| PositionRow {
            coin: p.coin.clone(),
            size: p.szi.clone(),
            entry_px: p.entry_px.clone(),
            position_value: p.position_value.clone(),
            unrealized_pnl: p.unrealized_pnl.clone(),
            leverage: format!("{}x {}", p.leverage.value, p.leverage.leverage_type),
            liquidation_px: p.liquidation_px.clone().unwrap_or_else(|| "-".to_string()),
        })
        .collect();

    if output_opts.json {
        output::print_json_pretty(&serde_json::json!({
            "positions": positions,
            "marginSummary": state.margin_summary,
            "crossMarginSummary": state.cross_margin_summary,
        }))?;
        return Ok(());
    }

    if positions.is_empty() {
        println!("{}", output::style_muted("No open positions"));
    } else {
        let rows: Vec<Vec<String>> = positions
            .iter()
            .map(|p| {
                vec![
                    p.coin.clone(),
                    p.size.clone(),
                    p.entry_px.clone().unwrap_or_else(|| "-".to_string()),
                    p.position_value.clone(),
                    format_pnl_cell(&p.unrealized_pnl),
                    p.leverage.clone(),
                    p.liquidation_px.clone(),
                ]
            })
            .collect();
        output::print_table_with_columns(&table_columns, rows);
    }
    println!();
    println!(
        "Account Value: {}",
        output::style_bold(&state.margin_summary.account_value)
    );
    println!(
        "Total Margin Used: {}",
        output::style_bold(&state.margin_summary.total_margin_used)
    );
    Ok(())
}

#[derive(serde::Serialize)]
struct OrderRow {
    oid: u64,
    coin: String,
    side: String,
    sz: String,
    #[serde(rename = "limitPx")]
    limit_px: String,
    timestamp: String,
}

fn format_order_timestamp(ms: u64) -> String {
    let Some(dt_utc) = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64) else {
        return ms.to_string();
    };

    let dt = dt_utc.with_timezone(&chrono::Local);

    let locale_raw = std::env::var("LC_ALL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("LC_TIME")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| std::env::var("LANG").ok().filter(|s| !s.trim().is_empty()));

    let locale = locale_raw
        .as_deref()
        .and_then(|s| s.split(['.', '@']).next())
        .map(|s| s.replace('_', "-"));

    match locale.as_deref() {
        Some(tag) if tag.eq_ignore_ascii_case("en-US") => {
            dt.format("%m/%d/%Y, %-I:%M:%S %p").to_string()
        }
        Some(tag) if tag.eq_ignore_ascii_case("en-AU") => dt
            .format("%d/%m/%Y, %-I:%M:%S %p")
            .to_string()
            .replace("AM", "am")
            .replace("PM", "pm"),
        Some(tag) if tag.eq_ignore_ascii_case("en-GB") => {
            dt.format("%d/%m/%Y, %H:%M:%S").to_string()
        }
        _ => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

async fn fetch_open_orders(api: &HlApi, user: Address) -> Result<Vec<OrderRow>> {
    let orders = api.open_orders(user).await?;
    let formatted: Vec<OrderRow> = orders
        .into_iter()
        .map(|o| OrderRow {
            oid: o.oid,
            coin: o.coin,
            side: o.side,
            sz: o.sz,
            limit_px: o.limit_px,
            timestamp: format_order_timestamp(o.timestamp),
        })
        .collect();
    Ok(formatted)
}

async fn account_orders(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: AccountUserWatchArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let user = resolve_user_address(cfg, &args.user)?;
    let table_columns = [
        output::TableColumn::right("OID"),
        output::TableColumn::left("Coin"),
        output::TableColumn::left("Side"),
        output::TableColumn::right("Size"),
        output::TableColumn::right("Price"),
        output::TableColumn::left("Time"),
    ];

    if args.watch {
        if !output_opts.json {
            watch::hide_cursor();
        }
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let mut last_updated = watch::format_timestamp();
        let mut orders: Vec<OrderRow> = Vec::new();

        let render = |orders: &[OrderRow], last_updated: &str| {
            watch::clear_screen();
            println!(
                "{}",
                output::format_watch_header("Open Orders", last_updated)
            );
            println!();
            if orders.is_empty() {
                println!("{}", output::style_muted("No open orders"));
            } else {
                let rows: Vec<Vec<String>> = orders
                    .iter()
                    .map(|o| {
                        let side = if o.side == "B" {
                            output::style_profit("Buy")
                        } else {
                            output::style_loss("Sell")
                        };
                        vec![
                            o.oid.to_string(),
                            o.coin.clone(),
                            side,
                            o.sz.clone(),
                            o.limit_px.clone(),
                            o.timestamp.clone(),
                        ]
                    })
                    .collect();
                output::print_table_with_columns(&table_columns, rows);
            }
            println!();
            println!("{}", output::style_muted("Press Ctrl+C to exit"));
        };

        if !output_opts.json {
            render(&orders, &last_updated);
        }

        match fetch_open_orders(api, user).await {
            Ok(initial) => {
                orders = initial;
                last_updated = watch::format_timestamp();
                if output_opts.json {
                    output::print_json_line(&serde_json::json!({
                        "orders": orders,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    }))?;
                } else {
                    render(&orders, &last_updated);
                }
            }
            Err(e) => {
                if !output_opts.json {
                    watch::clear_screen();
                    println!("{}", output::style_loss(format!("Error: {e}")));
                    println!("{}", output::style_muted("Reconnecting..."));
                } else {
                    output::print_error(e.to_string());
                }
            }
        }

        'outer: loop {
            let mut client = match ws::WsClient::connect(api.testnet).await {
                Ok(c) => c,
                Err(e) => {
                    if !output_opts.json {
                        watch::clear_screen();
                        println!("{}", output::style_loss(format!("Error: {e}")));
                        println!("{}", output::style_muted("Reconnecting..."));
                    } else {
                        output::print_error(e.to_string());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
            };

            if let Err(e) = client.subscribe(ws::sub_order_updates(user)).await {
                if !output_opts.json {
                    watch::clear_screen();
                    println!("{}", output::style_loss(format!("Error: {e}")));
                    println!("{}", output::style_muted("Reconnecting..."));
                } else {
                    output::print_error(e.to_string());
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }

            loop {
                tokio::select! {
                    msg = client.next_json() => {
                        let msg = match msg {
                            Ok(Some(m)) => m,
                            Ok(None) => break,
                            Err(e) => {
                                if !output_opts.json {
                                    watch::clear_screen();
                                    println!("{}", output::style_loss(format!("Error: {e}")));
                                    println!("{}", output::style_muted("Reconnecting..."));
                                } else {
                                    output::print_error(e.to_string());
                                }
                                break;
                            }
                        };

                        let Ok(channel) = ws::WsClient::channel(&msg) else {
                            continue;
                        };
                        if channel != "orderUpdates" {
                            continue;
                        }

                        match fetch_open_orders(api, user).await {
                            Ok(next) => {
                                orders = next;
                                last_updated = watch::format_timestamp();
                                if output_opts.json {
                                    output::print_json_line(&serde_json::json!({
                                        "orders": orders,
                                        "timestamp": chrono::Utc::now().to_rfc3339(),
                                    }))?;
                                } else {
                                    render(&orders, &last_updated);
                                }
                            }
                            Err(e) => {
                                if !output_opts.json {
                                    watch::clear_screen();
                                    println!("{}", output::style_loss(format!("Error: {e}")));
                                    println!("{}", output::style_muted("Reconnecting..."));
                                } else {
                                    output::print_error(e.to_string());
                                }
                            }
                        }
                    }
                    _ = &mut ctrl_c => break 'outer,
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let orders = fetch_open_orders(api, user).await?;
    if output_opts.json {
        output::print_json_pretty(&orders)?;
        return Ok(());
    }

    if orders.is_empty() {
        println!("{}", output::style_muted("No open orders"));
        return Ok(());
    }

    let rows: Vec<Vec<String>> = orders
        .iter()
        .map(|o| {
            let side = if o.side == "B" {
                output::style_profit("Buy")
            } else {
                output::style_loss("Sell")
            };
            vec![
                o.oid.to_string(),
                o.coin.clone(),
                side,
                o.sz.clone(),
                o.limit_px.clone(),
                o.timestamp.clone(),
            ]
        })
        .collect();
    output::print_table_with_columns(&table_columns, rows);
    Ok(())
}

#[derive(serde::Serialize)]
struct SpotBalanceRow {
    token: String,
    total: String,
    hold: String,
    available: String,
}

async fn fetch_balances(api: &HlApi, user: Address) -> Result<(Vec<SpotBalanceRow>, String)> {
    let (clearinghouse, spot_state) = tokio::try_join!(
        api.clearinghouse_state(user),
        api.spot_clearinghouse_state(user)
    )?;

    let spot_balances: Vec<SpotBalanceRow> = spot_state
        .balances
        .into_iter()
        .filter(|b| b.total.parse::<f64>().unwrap_or(0.0) != 0.0)
        .map(|b| {
            let total = b.total.parse::<f64>().unwrap_or(0.0);
            let hold = b.hold.parse::<f64>().unwrap_or(0.0);
            let available = total - hold;
            SpotBalanceRow {
                token: b.coin,
                total: b.total,
                hold: b.hold,
                available: available.to_string(),
            }
        })
        .collect();

    let perp_balance = clearinghouse.margin_summary.account_value.clone();
    Ok((spot_balances, perp_balance))
}

async fn account_balances(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: AccountUserWatchArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let user = resolve_user_address(cfg, &args.user)?;
    let table_columns = [
        output::TableColumn::left("Token"),
        output::TableColumn::right("Total"),
        output::TableColumn::right("Hold"),
        output::TableColumn::right("Available"),
    ];

    if args.watch {
        if !output_opts.json {
            watch::hide_cursor();
        }
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let mut last_updated = watch::format_timestamp();
        let mut spot_balances: Vec<SpotBalanceRow> = Vec::new();
        let mut perp_balance = "0".to_string();

        let render = |spot_balances: &[SpotBalanceRow], perp_balance: &str, last_updated: &str| {
            watch::clear_screen();
            println!("{}", output::format_watch_header("Balances", last_updated));
            println!();
            println!(
                "{}{} USD",
                output::style_bold("Perpetuals Balance: "),
                perp_balance
            );
            println!();
            println!("{}", output::style_header("Spot Balances:"));
            if spot_balances.is_empty() {
                println!("{}", output::style_muted("No spot balances"));
            } else {
                let rows: Vec<Vec<String>> = spot_balances
                    .iter()
                    .map(|b| {
                        vec![
                            b.token.clone(),
                            b.total.clone(),
                            b.hold.clone(),
                            b.available.clone(),
                        ]
                    })
                    .collect();
                output::print_table_with_columns(&table_columns, rows);
            }
            println!();
            println!("{}", output::style_muted("Press Ctrl+C to exit"));
        };

        if !output_opts.json {
            render(&spot_balances, &perp_balance, &last_updated);
        }

        'outer: loop {
            let mut client = match ws::WsClient::connect(api.testnet).await {
                Ok(c) => c,
                Err(e) => {
                    if !output_opts.json {
                        watch::clear_screen();
                        println!("{}", output::style_loss(format!("Error: {e}")));
                        println!("{}", output::style_muted("Reconnecting..."));
                    } else {
                        output::print_error(e.to_string());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
            };

            if let Err(e) = client
                .subscribe(ws::sub_all_dexs_clearinghouse_state(user))
                .await
            {
                if !output_opts.json {
                    watch::clear_screen();
                    println!("{}", output::style_loss(format!("Error: {e}")));
                    println!("{}", output::style_muted("Reconnecting..."));
                } else {
                    output::print_error(e.to_string());
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }

            loop {
                tokio::select! {
                    msg = client.next_json() => {
                        let msg = match msg {
                            Ok(Some(m)) => m,
                            Ok(None) => break,
                            Err(e) => {
                                if !output_opts.json {
                                    watch::clear_screen();
                                    println!("{}", output::style_loss(format!("Error: {e}")));
                                    println!("{}", output::style_muted("Reconnecting..."));
                                } else {
                                    output::print_error(e.to_string());
                                }
                                break;
                            }
                        };

                        let Ok(channel) = ws::WsClient::channel(&msg) else {
                            continue;
                        };
                        if channel != "allDexsClearinghouseState" {
                            continue;
                        }

                        let Ok(data) = ws::WsClient::data(&msg) else {
                            continue;
                        };
                        let Ok(event) = serde_json::from_value::<AllDexsClearinghouseStateEvent>(data.clone()) else {
                            continue;
                        };

                        if let Some((_, main)) = event.clearinghouse_states.first() {
                            perp_balance = main.margin_summary.account_value.clone();
                        } else {
                            perp_balance = "0".to_string();
                        }

                        if let Ok(spot_state) = api.spot_clearinghouse_state(user).await {
                            spot_balances = spot_state
                                .balances
                                .into_iter()
                                .filter(|b| b.total.parse::<f64>().unwrap_or(0.0) != 0.0)
                                .map(|b| {
                                    let total = b.total.parse::<f64>().unwrap_or(0.0);
                                    let hold = b.hold.parse::<f64>().unwrap_or(0.0);
                                    let available = total - hold;
                                    SpotBalanceRow {
                                        token: b.coin,
                                        total: b.total,
                                        hold: b.hold,
                                        available: available.to_string(),
                                    }
                                })
                                .collect();
                        }

                        last_updated = watch::format_timestamp();

                        if output_opts.json {
                            output::print_json_line(&serde_json::json!({
                                "spotBalances": spot_balances,
                                "perpBalance": perp_balance,
                                "timestamp": chrono::Utc::now().to_rfc3339(),
                            }))?;
                        } else {
                            render(&spot_balances, &perp_balance, &last_updated);
                        }
                    }
                    _ = &mut ctrl_c => break 'outer,
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let (spot_balances, perp_balance) = fetch_balances(api, user).await?;
    if output_opts.json {
        output::print_json_pretty(&serde_json::json!({
            "spotBalances": spot_balances,
            "perpBalance": perp_balance,
        }))?;
        return Ok(());
    }

    println!(
        "{}{} USD",
        output::style_bold("Perpetuals Balance: "),
        perp_balance
    );
    println!();
    println!("{}", output::style_header("Spot Balances:"));
    if spot_balances.is_empty() {
        println!("{}", output::style_muted("No spot balances"));
        return Ok(());
    }
    let rows: Vec<Vec<String>> = spot_balances
        .iter()
        .map(|b| {
            vec![
                b.token.clone(),
                b.total.clone(),
                b.hold.clone(),
                b.available.clone(),
            ]
        })
        .collect();
    output::print_table_with_columns(&table_columns, rows);
    Ok(())
}

async fn account_portfolio(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: AccountUserWatchArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let user = resolve_user_address(cfg, &args.user)?;
    let position_table_columns = [
        output::TableColumn::left("Coin"),
        output::TableColumn::right("Size"),
        output::TableColumn::right("Entry"),
        output::TableColumn::right("Value"),
        output::TableColumn::right("PnL"),
        output::TableColumn::right("Leverage"),
    ];
    let balance_table_columns = [
        output::TableColumn::left("Token"),
        output::TableColumn::right("Total"),
        output::TableColumn::right("Hold"),
    ];

    if args.watch {
        if !output_opts.json {
            watch::hide_cursor();
        }
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let mut last_updated = watch::format_timestamp();
        let mut data = PortfolioData {
            positions: Vec::new(),
            spot_balances: Vec::new(),
            account_value: "0".to_string(),
            total_margin_used: "0".to_string(),
        };

        let render = |data: &PortfolioData, last_updated: &str| {
            watch::clear_screen();
            println!("{}", output::format_watch_header("Portfolio", last_updated));
            println!();
            println!("{}", output::style_header("Account Summary"));
            println!("Account Value: {}", output::style_bold(&data.account_value));
            println!(
                "Total Margin Used: {}",
                output::style_bold(&data.total_margin_used)
            );
            println!();
            println!("{}", output::style_header("Perpetual Positions:"));
            if data.positions.is_empty() {
                println!("{}", output::style_muted("No open positions"));
            } else {
                let rows: Vec<Vec<String>> = data
                    .positions
                    .iter()
                    .map(|p| {
                        vec![
                            p.coin.clone(),
                            p.size.clone(),
                            p.entry_px.clone().unwrap_or_else(|| "-".to_string()),
                            p.position_value.clone(),
                            format_pnl_cell(&p.unrealized_pnl),
                            p.leverage.clone(),
                        ]
                    })
                    .collect();
                output::print_table_with_columns(&position_table_columns, rows);
            }
            println!();
            println!("{}", output::style_header("Spot Balances:"));
            if data.spot_balances.is_empty() {
                println!("{}", output::style_muted("No spot balances"));
            } else {
                let rows: Vec<Vec<String>> = data
                    .spot_balances
                    .iter()
                    .map(|b| vec![b.token.clone(), b.total.clone(), b.hold.clone()])
                    .collect();
                output::print_table_with_columns(&balance_table_columns, rows);
            }
            println!();
            println!("{}", output::style_muted("Press Ctrl+C to exit"));
        };

        if !output_opts.json {
            render(&data, &last_updated);
        }

        'outer: loop {
            let mut client = match ws::WsClient::connect(api.testnet).await {
                Ok(c) => c,
                Err(e) => {
                    if !output_opts.json {
                        watch::clear_screen();
                        println!("{}", output::style_loss(format!("Error: {e}")));
                        println!("{}", output::style_muted("Reconnecting..."));
                    } else {
                        output::print_error(e.to_string());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
            };

            if let Err(e) = client
                .subscribe(ws::sub_all_dexs_clearinghouse_state(user))
                .await
            {
                if !output_opts.json {
                    watch::clear_screen();
                    println!("{}", output::style_loss(format!("Error: {e}")));
                    println!("{}", output::style_muted("Reconnecting..."));
                } else {
                    output::print_error(e.to_string());
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }

            loop {
                tokio::select! {
                    msg = client.next_json() => {
                        let msg = match msg {
                            Ok(Some(m)) => m,
                            Ok(None) => break,
                            Err(e) => {
                                if !output_opts.json {
                                    watch::clear_screen();
                                    println!("{}", output::style_loss(format!("Error: {e}")));
                                    println!("{}", output::style_muted("Reconnecting..."));
                                } else {
                                    output::print_error(e.to_string());
                                }
                                break;
                            }
                        };

                        let Ok(channel) = ws::WsClient::channel(&msg) else {
                            continue;
                        };
                        if channel != "allDexsClearinghouseState" {
                            continue;
                        }

                        let Ok(data_msg) = ws::WsClient::data(&msg) else {
                            continue;
                        };
                        let Ok(event) = serde_json::from_value::<AllDexsClearinghouseStateEvent>(data_msg.clone()) else {
                            continue;
                        };

                        if let Some((_, main)) = event.clearinghouse_states.first() {
                            data.account_value = main.margin_summary.account_value.clone();
                            data.total_margin_used = main.margin_summary.total_margin_used.clone();
                        } else {
                            data.account_value = "0".to_string();
                            data.total_margin_used = "0".to_string();
                        }

                        data.positions = event
                            .clearinghouse_states
                            .into_iter()
                            .flat_map(|(_, st)| st.asset_positions.into_iter().map(|ap| ap.position))
                            .filter(|p| p.szi.parse::<f64>().unwrap_or(0.0) != 0.0)
                            .map(|p| PortfolioPositionRow {
                                coin: p.coin,
                                size: p.szi,
                                entry_px: p.entry_px,
                                position_value: p.position_value,
                                unrealized_pnl: p.unrealized_pnl,
                                leverage: format!("{}x {}", p.leverage.value, p.leverage.leverage_type),
                            })
                            .collect();

                        if let Ok(spot_state) = api.spot_clearinghouse_state(user).await {
                            data.spot_balances = spot_state
                                .balances
                                .into_iter()
                                .filter(|b| b.total.parse::<f64>().unwrap_or(0.0) != 0.0)
                                .map(|b| PortfolioSpotBalanceRow {
                                    token: b.coin,
                                    total: b.total,
                                    hold: b.hold,
                                })
                                .collect();
                        }

                        last_updated = watch::format_timestamp();

                        if output_opts.json {
                            let mut v = serde_json::to_value(&data)?;
                            if let Some(obj) = v.as_object_mut() {
                                obj.insert(
                                    "timestamp".to_string(),
                                    serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
                                );
                            }
                            output::print_json_line(&v)?;
                        } else {
                            render(&data, &last_updated);
                        }
                    }
                    _ = &mut ctrl_c => break 'outer,
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let data = fetch_portfolio(api, user).await?;
    if output_opts.json {
        output::print_json_pretty(&data)?;
        return Ok(());
    }

    println!("{}", output::style_header("Account Summary"));
    println!("Account Value: {}", output::style_bold(&data.account_value));
    println!(
        "Total Margin Used: {}",
        output::style_bold(&data.total_margin_used)
    );
    println!();
    println!("{}", output::style_header("Perpetual Positions:"));
    if data.positions.is_empty() {
        println!("{}", output::style_muted("No open positions"));
    } else {
        let rows: Vec<Vec<String>> = data
            .positions
            .iter()
            .map(|p| {
                vec![
                    p.coin.clone(),
                    p.size.clone(),
                    p.entry_px.clone().unwrap_or_else(|| "-".to_string()),
                    p.position_value.clone(),
                    format_pnl_cell(&p.unrealized_pnl),
                    p.leverage.clone(),
                ]
            })
            .collect();
        output::print_table_with_columns(&position_table_columns, rows);
    }

    println!();
    println!("{}", output::style_header("Spot Balances:"));
    if data.spot_balances.is_empty() {
        println!("{}", output::style_muted("No spot balances"));
    } else {
        let rows: Vec<Vec<String>> = data
            .spot_balances
            .iter()
            .map(|b| vec![b.token.clone(), b.total.clone(), b.hold.clone()])
            .collect();
        output::print_table_with_columns(&balance_table_columns, rows);
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct PortfolioPositionRow {
    coin: String,
    size: String,
    #[serde(rename = "entryPx")]
    entry_px: Option<String>,
    #[serde(rename = "positionValue")]
    position_value: String,
    #[serde(rename = "unrealizedPnl")]
    unrealized_pnl: String,
    leverage: String,
}

#[derive(serde::Serialize)]
struct PortfolioSpotBalanceRow {
    token: String,
    total: String,
    hold: String,
}

#[derive(serde::Serialize)]
struct PortfolioData {
    positions: Vec<PortfolioPositionRow>,
    #[serde(rename = "spotBalances")]
    spot_balances: Vec<PortfolioSpotBalanceRow>,
    #[serde(rename = "accountValue")]
    account_value: String,
    #[serde(rename = "totalMarginUsed")]
    total_margin_used: String,
}

async fn fetch_portfolio(api: &HlApi, user: Address) -> Result<PortfolioData> {
    let (clearinghouse, spot_state) = tokio::try_join!(
        api.clearinghouse_state(user),
        api.spot_clearinghouse_state(user)
    )?;

    let positions: Vec<PortfolioPositionRow> = clearinghouse
        .asset_positions
        .into_iter()
        .map(|p| p.position)
        .filter(|p| p.szi.parse::<f64>().unwrap_or(0.0) != 0.0)
        .map(|p| PortfolioPositionRow {
            coin: p.coin,
            size: p.szi,
            entry_px: p.entry_px,
            position_value: p.position_value,
            unrealized_pnl: p.unrealized_pnl,
            leverage: format!("{}x {}", p.leverage.value, p.leverage.leverage_type),
        })
        .collect();

    let spot_balances: Vec<PortfolioSpotBalanceRow> = spot_state
        .balances
        .into_iter()
        .filter(|b| b.total.parse::<f64>().unwrap_or(0.0) != 0.0)
        .map(|b| PortfolioSpotBalanceRow {
            token: b.coin,
            total: b.total,
            hold: b.hold,
        })
        .collect();

    Ok(PortfolioData {
        positions,
        spot_balances,
        account_value: clearinghouse.margin_summary.account_value,
        total_margin_used: clearinghouse.margin_summary.total_margin_used,
    })
}

async fn run_markets(cmd: MarketsCommand, api: &HlApi, output_opts: OutputOptions) -> Result<()> {
    match cmd {
        MarketsCommand::Ls(args) => markets_ls(api, args, output_opts).await,
        MarketsCommand::Prices(args) => markets_prices(api, args, output_opts).await,
    }
}

#[derive(serde::Serialize)]
struct MarketRow {
    coin: String,
    #[serde(rename = "pairName")]
    pair_name: String,
    price: String,
    #[serde(rename = "priceChange")]
    price_change: Option<f64>,
    #[serde(rename = "volumeUsd")]
    volume_usd: String,
    funding: Option<String>,
    #[serde(rename = "openInterest")]
    open_interest: Option<String>,
}

fn calculate_price_change(current: &str, prev: &str) -> Option<f64> {
    let c = current.parse::<f64>().ok()?;
    let p = prev.parse::<f64>().ok()?;
    if p == 0.0 {
        return None;
    }
    Some(((c - p) / p) * 100.0)
}

fn format_volume(volume: Option<&str>) -> String {
    let Some(volume) = volume else {
        return "N/A".to_string();
    };
    let volume = volume.trim();
    if volume.is_empty() {
        return "N/A".to_string();
    }
    let num = match volume.parse::<f64>() {
        Ok(v) if v.is_finite() => v,
        _ => return "N/A".to_string(),
    };

    // Match `toLocaleString(undefined, { maximumFractionDigits: 2 })` behavior.
    let mut s = format!("{num:.2}");
    if let Some(dot) = s.find('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.len() == dot + 1 {
            s.pop();
        }
    }

    let (sign, rest) = s
        .strip_prefix('-')
        .map(|r| ("-", r))
        .unwrap_or(("", s.as_str()));
    let (int_part, frac_part) = rest.split_once('.').unwrap_or((rest, ""));

    let mut grouped = String::with_capacity(rest.len() + rest.len() / 3);
    for (idx, ch) in int_part.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let int_grouped: String = grouped.chars().rev().collect();

    if frac_part.is_empty() {
        format!("{sign}{int_grouped}")
    } else {
        format!("{sign}{int_grouped}.{frac_part}")
    }
}

async fn markets_ls(_api: &HlApi, args: MarketsLsArgs, output_opts: OutputOptions) -> Result<()> {
    let is_spot_only = args.spot_only;
    let is_perp_only = args.perp_only;
    let table_columns = [
        output::TableColumn::left("Coin"),
        output::TableColumn::left("Pair"),
        output::TableColumn::right("Price"),
        output::TableColumn::right("24h %"),
        output::TableColumn::right("24h Volume"),
        output::TableColumn::right("Funding"),
        output::TableColumn::right("Open Interest"),
    ];

    if args.watch {
        if !output_opts.json {
            watch::hide_cursor();
        }
        let interval = tokio::time::interval(std::time::Duration::from_millis(250));
        tokio::pin!(interval);
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let mut client: Option<ServerClient> = None;
        let mut last_updated = watch::format_timestamp();
        let mut error: Option<String> = None;
        let mut perp_markets: Vec<MarketRow> = Vec::new();
        let mut spot_markets: Vec<MarketRow> = Vec::new();

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if client.is_none() {
                        client = ServerClient::try_connect().await?;
                        if client.is_none() {
                            error = Some("Server not running. Start with: rhl server start".to_string());
                        }
                    }

                    if let Some(c) = client.as_mut() {
                        match fetch_market_data(c, is_spot_only, is_perp_only).await {
                            Ok((next_perp_markets, next_spot_markets)) => {
                                perp_markets = next_perp_markets;
                                spot_markets = next_spot_markets;
                                last_updated = watch::format_timestamp();
                                error = None;

                                if output_opts.json {
                                    output::print_json_line(&serde_json::json!({
                                        "perpMarkets": perp_markets,
                                        "spotMarkets": spot_markets,
                                        "timestamp": chrono::Utc::now().to_rfc3339(),
                                    }))?;
                                    continue;
                                }
                            }
                            Err(e) => {
                                client = None;
                                error = Some(e.to_string());
                            }
                        }
                    }

                    if output_opts.json {
                        continue;
                    }

                    watch::clear_screen();
                    println!(
                        "{}",
                        output::format_watch_header(
                            format!(
                                "All Markets ({} perps, {} spot)",
                                perp_markets.len(),
                                spot_markets.len()
                            ),
                            &last_updated
                        )
                    );
                    println!();
                    if let Some(err) = &error {
                        println!(
                            "{} {}",
                            output::style_loss(format!("Error: {err}")),
                            output::style_muted("(reconnecting...)")
                        );
                        println!();
                    }

                    let mut rows = Vec::new();
                    for m in perp_markets.iter().chain(spot_markets.iter()) {
                        let price_change = m.price_change.map_or_else(
                            || output::style_muted("-"),
                            |c| {
                                let formatted = format!("{:+.2}%", c);
                                if c >= 0.0 {
                                    output::style_profit(formatted)
                                } else {
                                    output::style_loss(formatted)
                                }
                            },
                        );
                        let funding = m.funding.as_ref().map_or_else(
                            || output::style_muted("-"),
                            |f| {
                                let pct = f.parse::<f64>().ok().map(|x| x * 100.0);
                                pct.map_or_else(
                                    || output::style_muted("-"),
                                    |p| {
                                        let formatted = format!("{p:.4}%");
                                        if p >= 0.0 {
                                            output::style_profit(formatted)
                                        } else {
                                            output::style_loss(formatted)
                                        }
                                    },
                                )
                            },
                        );
                        let open_interest = m
                            .open_interest
                            .as_ref()
                            .map(|s| s.clone())
                            .unwrap_or_else(|| output::style_muted("-"));
                        rows.push(vec![
                            m.coin.clone(),
                            m.pair_name.clone(),
                            m.price.clone(),
                            price_change,
                            m.volume_usd.clone(),
                            funding,
                            open_interest,
                        ]);
                    }
                    if rows.is_empty() {
                        println!("{}", output::style_muted("No data"));
                    } else {
                        output::print_table_with_columns(&table_columns, rows);
                    }

                    println!();
                    println!("{}", output::style_muted("Press Ctrl+C to exit"));
                }
                _ = &mut ctrl_c => break,
            }
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let Some(mut client) = ServerClient::try_connect().await? else {
        return Err(anyhow!("Server not running. Start with: rhl server start"));
    };

    let (perp_markets, spot_markets) =
        fetch_market_data(&mut client, is_spot_only, is_perp_only).await?;
    if output_opts.json {
        output::print_json_pretty(&serde_json::json!({
            "perpMarkets": perp_markets,
            "spotMarkets": spot_markets,
        }))?;
        return Ok(());
    }

    println!(
        "{}",
        output::style_header(format!(
            "All Markets ({} perps, {} spot):",
            perp_markets.len(),
            spot_markets.len()
        ))
    );

    let mut rows = Vec::new();
    for m in perp_markets.iter().chain(spot_markets.iter()) {
        let price_change = m.price_change.map_or_else(
            || output::style_muted("-"),
            |c| {
                let formatted = format!("{:+.2}%", c);
                if c >= 0.0 {
                    output::style_profit(formatted)
                } else {
                    output::style_loss(formatted)
                }
            },
        );
        let funding = m.funding.as_ref().map_or_else(
            || output::style_muted("-"),
            |f| {
                let pct = f.parse::<f64>().ok().map(|x| x * 100.0);
                pct.map_or_else(
                    || output::style_muted("-"),
                    |p| {
                        let formatted = format!("{p:.4}%");
                        if p >= 0.0 {
                            output::style_profit(formatted)
                        } else {
                            output::style_loss(formatted)
                        }
                    },
                )
            },
        );
        let open_interest = m
            .open_interest
            .as_ref()
            .map(|s| s.clone())
            .unwrap_or_else(|| output::style_muted("-"));
        rows.push(vec![
            m.coin.clone(),
            m.pair_name.clone(),
            m.price.clone(),
            price_change,
            m.volume_usd.clone(),
            funding,
            open_interest,
        ]);
    }
    output::print_table_with_columns(&table_columns, rows);
    Ok(())
}

async fn fetch_market_data(
    client: &mut ServerClient,
    spot_only: bool,
    perp_only: bool,
) -> Result<(Vec<MarketRow>, Vec<MarketRow>)> {
    let spot_meta = client.get_spot_meta().await?.data;

    let mut perp_markets = Vec::new();
    if !spot_only {
        let all_perp_metas = client.get_perp_meta().await?.data;
        let asset_ctxs = client.get_asset_ctxs().await?.data;

        for (meta_index, perp_meta) in all_perp_metas.iter().enumerate() {
            let dex = if meta_index == 0 {
                Some(String::new())
            } else {
                perp_meta
                    .universe
                    .first()
                    .and_then(|m| m.name.split_once(':').map(|(p, _)| p.to_string()))
            };

            // Skip unexpected meta objects with no universe.
            let Some(dex) = dex else {
                continue;
            };

            let collateral = spot_meta
                .tokens
                .get(perp_meta.collateral_token as usize)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "?".to_string());
            let dex_suffix = if meta_index == 0 {
                String::new()
            } else {
                format!(" @{dex}")
            };
            let dex_ctxs = asset_ctxs
                .ctxs
                .iter()
                .find(|(name, _)| name == &dex)
                .map(|(_, ctxs)| ctxs);

            for (idx, market) in perp_meta.universe.iter().enumerate() {
                let ctx = dex_ctxs.and_then(|ctxs| ctxs.get(idx));
                let display_name = if meta_index == 0 {
                    market.name.as_str()
                } else {
                    market
                        .name
                        .split_once(':')
                        .map(|(_, rest)| rest)
                        .unwrap_or(market.name.as_str())
                };
                perp_markets.push(MarketRow {
                    coin: market.name.clone(),
                    pair_name: format!(
                        "{display_name}/{collateral} {}x{dex_suffix}",
                        market.max_leverage
                    ),
                    price: ctx
                        .map(|c| c.mark_px.clone())
                        .unwrap_or_else(|| "?".to_string()),
                    price_change: ctx
                        .and_then(|c| calculate_price_change(&c.mark_px, &c.prev_day_px)),
                    volume_usd: ctx
                        .map(|c| format_volume(Some(&c.day_ntl_vlm)))
                        .unwrap_or_else(|| "N/A".to_string()),
                    funding: ctx.map(|c| c.funding.clone()),
                    open_interest: ctx.and_then(|c| {
                        if c.open_interest.trim().is_empty() {
                            None
                        } else {
                            Some(format_volume(Some(&c.open_interest)))
                        }
                    }),
                });
            }
        }
    }

    let mut spot_markets = Vec::new();
    if !perp_only {
        let spot_ctxs = client.get_spot_asset_ctxs().await?.data;
        let mut ctx_map = std::collections::HashMap::new();
        for ctx in spot_ctxs {
            ctx_map.insert(ctx.coin.clone(), ctx);
        }
        for pair in spot_meta.universe.iter() {
            let base = spot_meta
                .tokens
                .get(pair.tokens.first().copied().unwrap_or_default() as usize)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "?".to_string());
            let quote = spot_meta
                .tokens
                .get(pair.tokens.get(1).copied().unwrap_or_default() as usize)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "?".to_string());
            let ctx = ctx_map.get(&pair.name);
            spot_markets.push(MarketRow {
                coin: pair.name.clone(),
                pair_name: format!("[Spot] {base}/{quote}"),
                price: ctx
                    .map(|c| c.mark_px.clone())
                    .unwrap_or_else(|| "?".to_string()),
                price_change: ctx.and_then(|c| calculate_price_change(&c.mark_px, &c.prev_day_px)),
                volume_usd: ctx
                    .map(|c| format_volume(Some(&c.day_ntl_vlm)))
                    .unwrap_or_else(|| "N/A".to_string()),
                funding: None,
                open_interest: None,
            });
        }
    }

    Ok((perp_markets, spot_markets))
}

async fn markets_prices(
    api: &HlApi,
    args: MarketsPricesArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    if args.watch {
        if !output_opts.json {
            watch::hide_cursor();
        }
        let interval = tokio::time::interval(std::time::Duration::from_millis(500));
        tokio::pin!(interval);
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match api.all_mids().await {
                        Ok(mids) => {
                            if output_opts.json {
                                output::print_json_line(&serde_json::json!({
                                    "mids": mids,
                                    "timestamp": chrono::Utc::now().to_rfc3339(),
                                }))?;
                            } else {
                                watch::clear_screen();
                                println!("Mid Prices ({}):", watch::format_timestamp());
                                let mut rows: Vec<Vec<String>> = mids.iter().map(|(k,v)| vec![k.clone(), v.clone()]).collect();
                                rows.sort_by(|a,b| a[0].cmp(&b[0]));
                                output::print_table(&["Coin","Price"], rows);
                            }
                        }
                        Err(e) => {
                            if !output_opts.json {
                                watch::clear_screen();
                                println!("Mid Prices ({}):", watch::format_timestamp());
                                println!("Error: {e}");
                                println!("Reconnecting...");
                            } else {
                                output::print_error(e.to_string());
                            }
                        }
                    }
                }
                _ = &mut ctrl_c => break,
            }
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let mids = api.all_mids().await?;
    if output_opts.json {
        output::print_json_pretty(&mids)?;
        return Ok(());
    }

    let mut rows: Vec<Vec<String>> = mids
        .iter()
        .map(|(k, v)| vec![k.clone(), v.clone()])
        .collect();
    rows.sort_by(|a, b| a[0].cmp(&b[0]));
    output::print_table(&["Coin", "Price"], rows);
    Ok(())
}

async fn run_asset(
    cmd: AssetCommand,
    cfg: &config::LoadedConfig,
    api: &HlApi,
    output_opts: OutputOptions,
) -> Result<()> {
    match cmd {
        AssetCommand::Price(args) => asset_price(api, args, output_opts).await,
        AssetCommand::Book(args) => asset_book(api, args, output_opts).await,
        AssetCommand::Leverage(args) => asset_leverage(cfg, api, args, output_opts).await,
    }
}

async fn asset_price(api: &HlApi, args: AssetPriceArgs, output_opts: OutputOptions) -> Result<()> {
    let coin = args.coin;

    if args.watch {
        if !output_opts.json {
            watch::hide_cursor();
        }

        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let mut price = "-".to_string();
        let mut last_updated = watch::format_timestamp();

        let render = |price: &str, last_updated: &str| {
            watch::clear_screen();
            println!(
                "{}",
                output::format_watch_header(format!("{coin} Price"), last_updated)
            );
            println!();
            println!(
                "{}: {}",
                output::style_header(&coin),
                output::style_bold(price)
            );
            println!();
            println!("{}", output::style_muted("Press Ctrl+C to exit"));
        };

        let mut server_client = ServerClient::try_connect().await?;

        if !output_opts.json {
            render(&price, &last_updated);
        }

        if let Some(c) = server_client.as_mut() {
            let interval = tokio::time::interval(std::time::Duration::from_millis(500));
            tokio::pin!(interval);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        match c.get_prices(None).await {
                            Ok(cached) => {
                                if let Some(next) = cached.data.get(&coin) {
                                    price = next.clone();
                                    last_updated = watch::format_timestamp();
                                    if output_opts.json {
                                        output::print_json_line(&serde_json::json!({
                                            "coin": coin,
                                            "price": price,
                                            "timestamp": chrono::Utc::now().to_rfc3339(),
                                        }))?;
                                    } else {
                                        render(&price, &last_updated);
                                    }
                                }
                            }
                            Err(e) => {
                                if !output_opts.json {
                                    watch::clear_screen();
                                    println!("{}", output::style_loss(format!("Error: {e}")));
                                    println!("{}", output::style_muted("Reconnecting..."));
                                } else {
                                    output::print_error(e.to_string());
                                }
                            }
                        }
                    }
                    _ = &mut ctrl_c => break,
                }
            }
        } else {
            'outer: loop {
                let mut client = match ws::WsClient::connect(api.testnet).await {
                    Ok(c) => c,
                    Err(e) => {
                        if !output_opts.json {
                            watch::clear_screen();
                            println!("{}", output::style_loss(format!("Error: {e}")));
                            println!("{}", output::style_muted("Reconnecting..."));
                        } else {
                            output::print_error(e.to_string());
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                };

                if let Err(e) = client.subscribe(ws::sub_all_mids_all_dexs()).await {
                    if !output_opts.json {
                        watch::clear_screen();
                        println!("{}", output::style_loss(format!("Error: {e}")));
                        println!("{}", output::style_muted("Reconnecting..."));
                    } else {
                        output::print_error(e.to_string());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }

                loop {
                    tokio::select! {
                        msg = client.next_json() => {
                            let msg = match msg {
                                Ok(Some(m)) => m,
                                Ok(None) => break,
                                Err(e) => {
                                    if !output_opts.json {
                                        watch::clear_screen();
                                        println!("{}", output::style_loss(format!("Error: {e}")));
                                        println!("{}", output::style_muted("Reconnecting..."));
                                    } else {
                                        output::print_error(e.to_string());
                                    }
                                    break;
                                }
                            };

                            let Ok(channel) = ws::WsClient::channel(&msg) else {
                                continue;
                            };
                            if channel == "allMids" {
                                let Ok(data) = ws::WsClient::data(&msg) else {
                                    continue;
                                };
                                let Some(mids) = data.get("mids").and_then(|m| m.as_object()) else {
                                    continue;
                                };
                                if let Some(next) = mids.get(&coin).and_then(|v| v.as_str()) {
                                    price = next.to_string();
                                    last_updated = watch::format_timestamp();
                                    if output_opts.json {
                                        output::print_json_line(&serde_json::json!({
                                            "coin": coin,
                                            "price": price,
                                            "timestamp": chrono::Utc::now().to_rfc3339(),
                                        }))?;
                                    } else {
                                        render(&price, &last_updated);
                                    }
                                }
                            }
                        }
                        _ = &mut ctrl_c => break 'outer,
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let mids = if let Some(mut client) = ServerClient::try_connect().await? {
        match client.get_prices(None).await {
            Ok(cached) => cached.data,
            Err(_) => api.all_mids().await?,
        }
    } else {
        api.all_mids().await?
    };
    let price = mids
        .get(&coin)
        .cloned()
        .ok_or_else(|| anyhow!("Coin not found: {coin}"))?;

    if output_opts.json {
        output::print_json_pretty(&serde_json::json!({ "coin": coin, "price": price }))?;
        return Ok(());
    }

    println!(
        "{}: {}",
        output::style_header(&coin),
        output::style_bold(price)
    );
    Ok(())
}

async fn asset_book(api: &HlApi, args: AssetBookArgs, output_opts: OutputOptions) -> Result<()> {
    let coin = args.coin;

    const MAX_LEVELS: usize = 10;
    const BAR_WIDTH: usize = 20;
    const PRICE_WIDTH: usize = 12;
    const SIZE_WIDTH: usize = 12;
    const ORDERS_WIDTH: usize = 4;

    fn pad_end(value: &str, width: usize) -> String {
        let len = value.chars().count();
        if len >= width {
            return value.to_string();
        }
        format!("{value}{}", " ".repeat(width - len))
    }

    fn create_depth_bar(ratio: f64, width: usize) -> String {
        let ratio = ratio.clamp(0.0, 1.0);
        let filled = ((ratio * width as f64).round() as usize).max(1).min(width);
        "█".repeat(filled)
    }

    let render = |book: &hyperliquid_cli::hl_api::L2Book, is_watch: bool| {
        let bids = book.levels.first().cloned().unwrap_or_default();
        let asks = book.levels.get(1).cloned().unwrap_or_default();

        if is_watch {
            println!(
                "{}",
                output::format_watch_header(
                    format!("{coin} Order Book"),
                    watch::format_timestamp()
                )
            );
        } else {
            println!("{}", output::style_header(format!("{coin} Order Book")));
        }
        println!();

        // Header
        println!(
            "{} {} {} {}",
            output::style_muted(pad_end("price", PRICE_WIDTH)),
            output::style_muted(pad_end("size", SIZE_WIDTH)),
            output::style_muted(pad_end("#", ORDERS_WIDTH)),
            output::style_muted(pad_end("depth", BAR_WIDTH)),
        );

        let display_bids: Vec<_> = bids.into_iter().take(MAX_LEVELS).collect();
        let asks_to_process: Vec<_> = asks.into_iter().take(MAX_LEVELS).collect();

        // Asks: cumulative from best ask (lowest) then reverse for display.
        let mut ask_cumulative = 0.0_f64;
        let mut asks_with_cumulative: Vec<(hyperliquid_cli::hl_api::BookLevel, f64)> =
            asks_to_process
                .into_iter()
                .map(|level| {
                    ask_cumulative += level.sz.parse::<f64>().unwrap_or(0.0);
                    (level, ask_cumulative)
                })
                .collect();
        asks_with_cumulative.reverse();

        // Bids: cumulative in display order (best bid first).
        let mut bid_cumulative = 0.0_f64;
        let bids_with_cumulative: Vec<(hyperliquid_cli::hl_api::BookLevel, f64)> = display_bids
            .into_iter()
            .map(|level| {
                bid_cumulative += level.sz.parse::<f64>().unwrap_or(0.0);
                (level, bid_cumulative)
            })
            .collect();

        let max_cumulative = asks_with_cumulative
            .first()
            .map(|(_, c)| *c)
            .unwrap_or(0.0)
            .max(bids_with_cumulative.last().map(|(_, c)| *c).unwrap_or(0.0));

        if asks_with_cumulative.is_empty() {
            println!("{}", output::style_muted("No asks"));
        }

        for (level, cumulative) in &asks_with_cumulative {
            let bar = if max_cumulative > 0.0 {
                create_depth_bar(*cumulative / max_cumulative, BAR_WIDTH)
            } else {
                create_depth_bar(0.0, BAR_WIDTH)
            };
            println!(
                "{} {} {} {}",
                output::style_loss(pad_end(&level.px, PRICE_WIDTH)),
                pad_end(&level.sz, SIZE_WIDTH),
                output::style_muted(pad_end(&level.n.to_string(), ORDERS_WIDTH)),
                output::style_loss(bar)
            );
        }

        if let (Some((best_ask, _)), Some((best_bid, _))) =
            (asks_with_cumulative.last(), bids_with_cumulative.first())
            && let (Ok(ask_px), Ok(bid_px)) =
                (best_ask.px.parse::<f64>(), best_bid.px.parse::<f64>())
        {
            let spread = ask_px - bid_px;
            let total_width = PRICE_WIDTH + SIZE_WIDTH + ORDERS_WIDTH + BAR_WIDTH + 6;
            let side = "─".repeat(total_width / 2);
            println!(
                "{}",
                output::style_warning(format!("{side} spread: {spread:.2} {side}"))
            );
        }

        if bids_with_cumulative.is_empty() {
            println!("{}", output::style_muted("No bids"));
        }

        for (level, cumulative) in &bids_with_cumulative {
            let bar = if max_cumulative > 0.0 {
                create_depth_bar(*cumulative / max_cumulative, BAR_WIDTH)
            } else {
                create_depth_bar(0.0, BAR_WIDTH)
            };
            println!(
                "{} {} {} {}",
                output::style_profit(pad_end(&level.px, PRICE_WIDTH)),
                pad_end(&level.sz, SIZE_WIDTH),
                output::style_muted(pad_end(&level.n.to_string(), ORDERS_WIDTH)),
                output::style_profit(bar)
            );
        }

        if is_watch {
            println!();
            println!("{}", output::style_muted("Press Ctrl+C to exit"));
        }
    };

    if args.watch {
        if !output_opts.json {
            watch::hide_cursor();
        }
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        'outer: loop {
            let mut client = match ws::WsClient::connect(api.testnet).await {
                Ok(c) => c,
                Err(e) => {
                    if !output_opts.json {
                        watch::clear_screen();
                        println!("{}", output::style_loss(format!("Error: {e}")));
                        println!("{}", output::style_muted("Reconnecting..."));
                    } else {
                        output::print_error(e.to_string());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
            };

            if let Err(e) = client.subscribe(ws::sub_l2_book(&coin)).await {
                if !output_opts.json {
                    watch::clear_screen();
                    println!("{}", output::style_loss(format!("Error: {e}")));
                    println!("{}", output::style_muted("Reconnecting..."));
                } else {
                    output::print_error(e.to_string());
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }

            loop {
                tokio::select! {
                    msg = client.next_json() => {
                        let msg = match msg {
                            Ok(Some(m)) => m,
                            Ok(None) => break,
                            Err(e) => {
                                if !output_opts.json {
                                    watch::clear_screen();
                                    println!("{}", output::style_loss(format!("Error: {e}")));
                                    println!("{}", output::style_muted("Reconnecting..."));
                                } else {
                                    output::print_error(e.to_string());
                                }
                                break;
                            }
                        };

                        let Ok(channel) = ws::WsClient::channel(&msg) else {
                            continue;
                        };
                        if channel == "l2Book" {
                            let Ok(data) = ws::WsClient::data(&msg) else {
                                continue;
                            };
                            let Ok(book) = serde_json::from_value::<hyperliquid_cli::hl_api::L2Book>(data.clone()) else {
                                continue;
                            };

                            if output_opts.json {
                                output::print_json_line(&serde_json::json!({
                                    "coin": book.coin,
                                    "bids": book.levels.first().cloned().unwrap_or_default(),
                                    "asks": book.levels.get(1).cloned().unwrap_or_default(),
                                    "time": book.time,
                                    "timestamp": chrono::Utc::now().to_rfc3339(),
                                }))?;
                            } else {
                                watch::clear_screen();
                                render(&book, true);
                            }
                        }
                    }
                    _ = &mut ctrl_c => break 'outer,
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let book = api.l2_book(&coin).await?;
    if output_opts.json {
        output::print_json_pretty(&book)?;
        return Ok(());
    }

    render(&book, false);
    Ok(())
}

async fn asset_leverage(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: AssetLeverageArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let user = resolve_user_address(cfg, &args.user)?;
    let coin = args.coin;

    let render = |info: &LeverageInfo, is_watch: bool| {
        if is_watch {
            println!(
                "{}",
                output::format_watch_header(
                    format!("{} Leverage", info.coin),
                    watch::format_timestamp()
                )
            );
            println!();
        }

        println!(
            "{}",
            output::style_header(format!("{} Leverage Info", info.coin))
        );
        println!(
            "Leverage: {}",
            output::style_bold(format!(
                "{}x {}",
                info.leverage.value, info.leverage.leverage_type
            ))
        );
        println!(
            "Max Leverage: {}",
            output::style_bold(format!("{}x", info.max_leverage))
        );
        println!(
            "Mark Price: {}",
            output::style_bold(format!("${}", info.mark_px))
        );
        println!();

        println!("{}", output::style_header("Position"));
        if let Some(pos) = &info.position {
            println!("Size: {}", output::style_bold(&pos.size));
            println!("Value: {}", output::style_bold(format!("${}", pos.value)));
        } else {
            println!("{}", output::style_muted("No position"));
        }
        println!();

        println!("{}", output::style_header("Trading Capacity"));
        println!(
            "Available to Trade: {}",
            output::style_profit_bold(format!(
                "{} / {}",
                info.available_to_trade[0], info.available_to_trade[1]
            ))
        );
        println!(
            "Max Trade Size: {}",
            output::style_bold(format!(
                "{} / {}",
                info.max_trade_szs[0], info.max_trade_szs[1]
            ))
        );
        println!();

        println!("{}", output::style_header("Margin"));
        println!(
            "Account Value: {}",
            output::style_bold(format!("${}", info.margin.account_value))
        );
        println!(
            "Total Margin Used: {}",
            output::style_bold(format!("${}", info.margin.total_margin_used))
        );
        println!(
            "Available Margin: {}",
            output::style_profit_bold(format!("${}", info.margin.available_margin))
        );

        if is_watch {
            println!();
            println!("{}", output::style_muted("Press Ctrl+C to exit"));
        }
    };

    if args.watch {
        let mut info = fetch_leverage_info(api, user, &coin).await?;

        if !output_opts.json {
            watch::hide_cursor();
            watch::clear_screen();
            println!("{}", output::style_muted("Loading..."));
        }
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        'outer: loop {
            let mut client = match ws::WsClient::connect(api.testnet).await {
                Ok(c) => c,
                Err(e) => {
                    if !output_opts.json {
                        watch::clear_screen();
                        println!("{}", output::style_loss(format!("Error: {e}")));
                        println!("{}", output::style_muted("Reconnecting..."));
                    } else {
                        output::print_error(e.to_string());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
            };

            if let Err(e) = client
                .subscribe(ws::sub_active_asset_data(user, &coin))
                .await
            {
                if !output_opts.json {
                    watch::clear_screen();
                    println!("{}", output::style_loss(format!("Error: {e}")));
                    println!("{}", output::style_muted("Reconnecting..."));
                } else {
                    output::print_error(e.to_string());
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }

            loop {
                tokio::select! {
                    msg = client.next_json() => {
                        let msg = match msg {
                            Ok(Some(m)) => m,
                            Ok(None) => break,
                            Err(e) => {
                                if !output_opts.json {
                                    watch::clear_screen();
                                    println!("{}", output::style_loss(format!("Error: {e}")));
                                    println!("{}", output::style_muted("Reconnecting..."));
                                } else {
                                    output::print_error(e.to_string());
                                }
                                break;
                            }
                        };

                        let Ok(channel) = ws::WsClient::channel(&msg) else {
                            continue;
                        };
                        if channel != "activeAssetData" {
                            continue;
                        }

                        let Ok(data) = ws::WsClient::data(&msg) else {
                            continue;
                        };
                        let Ok(active) = serde_json::from_value::<hyperliquid_cli::hl_api::ActiveAssetData>(data.clone()) else {
                            continue;
                        };

                        info.coin = active.coin;
                        info.leverage = active.leverage;
                        info.max_trade_szs = active.max_trade_szs;
                        info.available_to_trade = active.available_to_trade;
                        info.mark_px = active.mark_px;

                        if output_opts.json {
                            let mut v = serde_json::to_value(&info)?;
                            if let Some(obj) = v.as_object_mut() {
                                obj.insert(
                                    "timestamp".to_string(),
                                    serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
                                );
                            }
                            output::print_json_line(&v)?;
                        } else {
                            watch::clear_screen();
                            render(&info, true);
                        }
                    }
                    _ = &mut ctrl_c => break 'outer,
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        if !output_opts.json {
            watch::show_cursor();
        }
        return Ok(());
    }

    let info = fetch_leverage_info(api, user, &coin).await?;
    if output_opts.json {
        output::print_json_pretty(&info)?;
        return Ok(());
    }

    render(&info, false);
    Ok(())
}

#[derive(serde::Serialize)]
struct LeveragePositionInfo {
    size: String,
    value: String,
}

#[derive(serde::Serialize)]
struct LeverageMarginInfo {
    #[serde(rename = "accountValue")]
    account_value: String,
    #[serde(rename = "totalMarginUsed")]
    total_margin_used: String,
    #[serde(rename = "availableMargin")]
    available_margin: String,
}

#[derive(serde::Serialize)]
struct LeverageInfo {
    coin: String,
    leverage: hyperliquid_cli::hl_api::Leverage,
    #[serde(rename = "maxLeverage")]
    max_leverage: u32,
    #[serde(rename = "markPx")]
    mark_px: String,
    #[serde(rename = "maxTradeSzs")]
    max_trade_szs: [String; 2],
    #[serde(rename = "availableToTrade")]
    available_to_trade: [String; 2],
    position: Option<LeveragePositionInfo>,
    margin: LeverageMarginInfo,
}

async fn fetch_leverage_info(api: &HlApi, user: Address, coin: &str) -> Result<LeverageInfo> {
    let (active, clearinghouse, all_perp_metas) = tokio::try_join!(
        api.active_asset_data(user, coin),
        api.clearinghouse_state(user),
        api.all_perp_metas(),
    )?;

    let mut max_leverage = 50;
    for dex in &all_perp_metas {
        if let Some(asset) = dex.universe.iter().find(|a| a.name == coin) {
            max_leverage = asset.max_leverage;
            break;
        }
    }

    let position = clearinghouse
        .asset_positions
        .iter()
        .map(|p| &p.position)
        .find(|p| p.coin == coin && p.szi.parse::<f64>().unwrap_or(0.0) != 0.0)
        .map(|p| LeveragePositionInfo {
            size: p.szi.clone(),
            value: p.position_value.clone(),
        });

    let account_value_f = clearinghouse
        .margin_summary
        .account_value
        .parse::<f64>()
        .unwrap_or(0.0);
    let total_margin_used_f = clearinghouse
        .margin_summary
        .total_margin_used
        .parse::<f64>()
        .unwrap_or(0.0);
    let available_margin = (account_value_f - total_margin_used_f).max(0.0);

    Ok(LeverageInfo {
        coin: coin.to_string(),
        leverage: active.leverage,
        max_leverage,
        mark_px: active.mark_px,
        max_trade_szs: active.max_trade_szs,
        available_to_trade: active.available_to_trade,
        position,
        margin: LeverageMarginInfo {
            account_value: clearinghouse.margin_summary.account_value,
            total_margin_used: clearinghouse.margin_summary.total_margin_used,
            available_margin: format!("{available_margin:.2}"),
        },
    })
}

async fn run_order(
    cmd: OrderCommand,
    cfg: &config::LoadedConfig,
    api: &HlApi,
    output_opts: OutputOptions,
) -> Result<()> {
    match cmd {
        OrderCommand::Ls(args) => account_orders(cfg, api, args, output_opts).await,
        OrderCommand::Configure(args) => order_configure(args, output_opts),
        OrderCommand::Limit(args) => order_limit(cfg, api, args, output_opts).await,
        OrderCommand::Market(args) => order_market(cfg, api, args, output_opts).await,
        OrderCommand::Cancel(args) => order_cancel(cfg, api, args, output_opts).await,
        OrderCommand::CancelAll(args) => order_cancel_all(cfg, api, args, output_opts).await,
        OrderCommand::SetLeverage(args) => order_set_leverage(cfg, api, args, output_opts).await,
    }
}

fn order_configure(args: OrderConfigureArgs, output_opts: OutputOptions) -> Result<()> {
    if let Some(s) = args.slippage {
        let slippage = validation::validate_non_negative_number(&s, "slippage")?;
        let cfg = order_config::update_order_config(Some(slippage))?;
        if output_opts.json {
            output::print_json_pretty(&cfg)?;
        } else {
            output::print_success(format!("Slippage set to {slippage}%"));
        }
        return Ok(());
    }

    let cfg = order_config::load_order_config();
    if output_opts.json {
        output::print_json_pretty(&cfg)?;
    } else {
        output::print_success("Current configuration:");
        println!("  Slippage: {}%", cfg.slippage);
    }
    Ok(())
}

async fn order_limit(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: OrderLimitArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let side = validation::validate_side_with_aliases(&args.side)?;
    let size = validation::validate_positive_number(&args.size, "size")?;
    let limit_px = validation::validate_positive_number(&args.price, "price")?;
    let tif = validation::validate_tif(&args.tif)?;
    let is_buy = side == "buy";

    let (all_perp_metas, spot_meta) = tokio::try_join!(api.all_perp_metas(), api.spot_meta())?;
    let asset = asset_index::resolve_asset_index(&all_perp_metas, &spot_meta, &args.coin)?;

    let wallet = require_wallet_signer(cfg)?;
    let exchange = Exchange::new(exchange_chain(cfg.testnet));
    let order = OrderRequest {
        asset,
        is_buy,
        limit_px: limit_px.to_string(),
        sz: size.to_string(),
        reduce_only: args.reduce_only,
        order_type: OrderType::Limit(Limit { tif }),
        cloid: None,
    };

    let resp = exchange.place_order(wallet, vec![order], None).await?;
    if output_opts.json {
        output::print_json_pretty(&resp)?;
    } else {
        print_exchange_response(&resp);
    }
    Ok(())
}

async fn order_market(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: OrderMarketArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let side = validation::validate_side_with_aliases(&args.side)?;
    let size = validation::validate_positive_number(&args.size, "size")?;
    let is_buy = side == "buy";

    let (all_perp_metas, spot_meta, mids) =
        tokio::try_join!(api.all_perp_metas(), api.spot_meta(), api.all_mids())?;
    let asset = asset_index::resolve_asset_index(&all_perp_metas, &spot_meta, &args.coin)?;

    let mid_price: f64 = mids
        .get(&args.coin)
        .ok_or_else(|| anyhow!("Cannot get mid price for {}", args.coin))?
        .parse()
        .map_err(|_| anyhow!("Invalid mid price for {}", args.coin))?;

    let config_slippage = order_config::load_order_config().slippage;
    let slippage_pct = if let Some(s) = args.slippage {
        validation::validate_non_negative_number(&s, "slippage")?
    } else {
        config_slippage
    } / 100.0;

    let limit_px = if is_buy {
        mid_price * (1.0 + slippage_pct)
    } else {
        mid_price * (1.0 - slippage_pct)
    };

    let wallet = require_wallet_signer(cfg)?;
    let exchange = Exchange::new(exchange_chain(cfg.testnet));
    let order = OrderRequest {
        asset,
        is_buy,
        limit_px: format!("{limit_px:.6}"),
        sz: size.to_string(),
        reduce_only: args.reduce_only,
        order_type: OrderType::Limit(Limit {
            tif: hyperliquid::types::exchange::request::Tif::Ioc,
        }),
        cloid: None,
    };

    let resp = exchange.place_order(wallet, vec![order], None).await?;
    if output_opts.json {
        output::print_json_pretty(&resp)?;
    } else {
        print_exchange_response(&resp);
    }
    Ok(())
}

async fn order_cancel(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: OrderCancelArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let user = require_wallet_address(cfg)?;
    let open_orders = api.open_orders(user).await?;
    if open_orders.is_empty() && args.oid.is_none() {
        output::print_success("No open orders to cancel");
        return Ok(());
    }

    let (order_id, coin) = if let Some(oid_arg) = args.oid {
        let oid = validation::validate_positive_u64(&oid_arg, "oid")?;
        let order = open_orders
            .iter()
            .find(|o| o.oid == oid)
            .ok_or_else(|| anyhow!("Order {oid} not found in open orders"))?;
        (oid, order.coin.clone())
    } else {
        let options = open_orders
            .iter()
            .map(|o| prompt::SelectOption {
                value: (o.oid, o.coin.clone()),
                label: format!(
                    "{}: {} {} {} @ {}",
                    o.oid,
                    o.coin,
                    if o.side == "B" { "Buy" } else { "Sell" },
                    o.sz,
                    o.limit_px
                ),
            })
            .collect();
        prompt::select("Select order to cancel:", options)?
    };

    let (all_perp_metas, spot_meta) = tokio::try_join!(api.all_perp_metas(), api.spot_meta())?;
    let asset = asset_index::resolve_asset_index(&all_perp_metas, &spot_meta, &coin)?;

    let wallet = require_wallet_signer(cfg)?;
    let exchange = Exchange::new(exchange_chain(cfg.testnet));
    let resp = exchange
        .cancel_order(
            wallet,
            vec![CancelRequest {
                asset,
                oid: order_id,
            }],
            None,
        )
        .await?;

    if output_opts.json {
        output::print_json_pretty(&resp)?;
    } else {
        output::print_success(format!("Order {order_id} cancelled"));
    }
    Ok(())
}

async fn order_cancel_all(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: OrderCancelAllArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let user = require_wallet_address(cfg)?;
    let open_orders = api.open_orders(user).await?;
    if open_orders.is_empty() {
        output::print_success("No open orders to cancel");
        return Ok(());
    }

    let mut orders_to_cancel = open_orders;
    if let Some(coin) = &args.coin {
        orders_to_cancel.retain(|o| &o.coin == coin);
        if orders_to_cancel.is_empty() {
            output::print_success(format!("No open orders for {coin}"));
            return Ok(());
        }
    }

    if !args.yes {
        let confirm_msg = if let Some(coin) = &args.coin {
            format!("Cancel all {} orders for {coin}?", orders_to_cancel.len())
        } else {
            format!("Cancel all {} open orders?", orders_to_cancel.len())
        };
        let confirmed = prompt::confirm(&confirm_msg, false)?;
        if !confirmed {
            output::print_success("Cancelled");
            return Ok(());
        }
    }

    let (all_perp_metas, spot_meta) = tokio::try_join!(api.all_perp_metas(), api.spot_meta())?;
    let cancels: Vec<CancelRequest> = orders_to_cancel
        .iter()
        .map(|o| {
            let asset = asset_index::resolve_asset_index(&all_perp_metas, &spot_meta, &o.coin)?;
            Ok(CancelRequest { asset, oid: o.oid })
        })
        .collect::<Result<Vec<_>>>()?;

    let wallet = require_wallet_signer(cfg)?;
    let exchange = Exchange::new(exchange_chain(cfg.testnet));
    let resp = exchange.cancel_order(wallet, cancels, None).await?;

    if output_opts.json {
        output::print_json_pretty(&resp)?;
    } else {
        output::print_success(format!("Cancelled {} orders", orders_to_cancel.len()));
    }
    Ok(())
}

async fn order_set_leverage(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    args: OrderSetLeverageArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let leverage = validation::validate_positive_u64(&args.leverage, "leverage")? as u32;

    let (all_perp_metas, spot_meta) = tokio::try_join!(api.all_perp_metas(), api.spot_meta())?;
    let asset = asset_index::resolve_asset_index(&all_perp_metas, &spot_meta, &args.coin)?;

    let is_cross = args.cross || !args.isolated;

    let wallet = require_wallet_signer(cfg)?;
    let exchange = Exchange::new(exchange_chain(cfg.testnet));
    let resp = exchange
        .update_leverage(wallet, leverage, asset, is_cross)
        .await?;

    if output_opts.json {
        output::print_json_pretty(&resp)?;
    } else {
        output::print_success(format!(
            "Leverage set to {leverage}x ({}) for {}",
            if is_cross { "cross" } else { "isolated" },
            args.coin
        ));
    }
    Ok(())
}

async fn run_referral(
    cmd: ReferralCommand,
    cfg: &config::LoadedConfig,
    api: &HlApi,
    output_opts: OutputOptions,
) -> Result<()> {
    match cmd {
        ReferralCommand::Set(args) => referral_set(cfg, args, output_opts).await,
        ReferralCommand::Status => referral_status(cfg, api, output_opts).await,
    }
}

async fn referral_set(
    cfg: &config::LoadedConfig,
    args: ReferralSetArgs,
    output_opts: OutputOptions,
) -> Result<()> {
    let wallet = require_wallet_signer(cfg)?;
    let exchange = Exchange::new(exchange_chain(cfg.testnet));
    let resp = exchange.set_referrer(wallet, args.code.clone()).await?;

    if output_opts.json {
        output::print_json_pretty(&resp)?;
    } else {
        output::print_success(format!("Referral code set: {}", args.code));
    }
    Ok(())
}

async fn referral_status(
    cfg: &config::LoadedConfig,
    api: &HlApi,
    output_opts: OutputOptions,
) -> Result<()> {
    let user = require_wallet_address(cfg)?;
    let result = api.referral(user).await?;
    if output_opts.json {
        output::print_json_pretty(&result)?;
    } else if result.is_null() {
        println!("No referral information found");
    } else {
        output::print_json_pretty(&result)?;
    }
    Ok(())
}

async fn run_server(cmd: ServerCommand, testnet: bool, output_opts: OutputOptions) -> Result<()> {
    match cmd {
        ServerCommand::Start => server_start(testnet, output_opts).await,
        ServerCommand::Stop => server_stop(output_opts).await,
        ServerCommand::Status => server_status(output_opts).await,
    }
}

fn format_uptime(ms: i64) -> String {
    let ms = ms.max(0) as u64;
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    if days > 0 {
        format!("{days}d {}h", hours % 24)
    } else if hours > 0 {
        format!("{hours}h {}m", minutes % 60)
    } else if minutes > 0 {
        format!("{minutes}m {}s", seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn format_age(ms: Option<i64>) -> String {
    let Some(ms) = ms else {
        return "unknown".to_string();
    };
    let ms = ms.max(0) as u64;
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let seconds = ms / 1000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    format!("{minutes}m {}s", seconds % 60)
}

#[cfg(test)]
mod server_format_tests {
    use super::*;

    #[test]
    fn format_uptime_matches_js_helpers() {
        assert_eq!(format_uptime(30_000), "30s");
        assert_eq!(format_uptime(90_000), "1m 30s");
        assert_eq!(format_uptime(3_660_000), "1h 1m");
        assert_eq!(format_uptime(90_000_000), "1d 1h");
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(60_000), "1m 0s");
        assert_eq!(format_uptime(3_600_000), "1h 0m");
        assert_eq!(format_uptime(86_400_000), "1d 0h");
        assert_eq!(format_uptime(172_800_000 + 3_600_000 * 5), "2d 5h");
    }

    #[test]
    fn format_age_matches_js_helpers() {
        assert_eq!(format_age(None), "unknown");
        assert_eq!(format_age(Some(500)), "500ms");
        assert_eq!(format_age(Some(30_000)), "30s");
        assert_eq!(format_age(Some(90_000)), "1m 30s");
        assert_eq!(format_age(Some(1_000)), "1s");
        assert_eq!(format_age(Some(60_000)), "1m 0s");
        assert_eq!(format_age(Some(0)), "0ms");
        assert_eq!(format_age(Some(999)), "999ms");
    }
}

fn find_rhl_server_exe() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe().context("Failed to locate current executable")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("Invalid current executable path"))?;
    let name = if cfg!(windows) {
        "rhl-server.exe"
    } else {
        "rhl-server"
    };
    let candidate = dir.join(name);
    if candidate.exists() {
        Ok(candidate)
    } else {
        Ok(std::path::PathBuf::from(name))
    }
}

async fn server_start(testnet: bool, output_opts: OutputOptions) -> Result<()> {
    if ServerClient::try_connect().await?.is_some() {
        let pid = paths::server_pid_path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| s.trim().parse::<i32>().ok());
        if let Some(pid) = pid {
            return Err(anyhow!("Server is already running (pid: {pid})"));
        }
        return Err(anyhow!("Server is already running"));
    }

    // If the socket file exists but the server isn't responsive, treat it as stale.
    if ServerClient::is_server_running()?
        && let Ok(p) = paths::server_socket_path()
    {
        let _ = std::fs::remove_file(p);
    }

    let server_exe = find_rhl_server_exe()?;
    let mut cmd = std::process::Command::new(server_exe);
    if testnet {
        cmd.arg("--testnet");
    }
    cmd.arg("--daemonize");
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn().context("Failed to start rhl-server")?;

    // Wait for server to start (max 10s)
    let max_wait = std::time::Duration::from_secs(10);
    let poll = std::time::Duration::from_millis(200);
    let mut waited = std::time::Duration::from_millis(0);
    while waited < max_wait {
        tokio::time::sleep(poll).await;
        waited += poll;
        if let Some(mut client) = ServerClient::try_connect().await? {
            let status = client.get_status().await?;
            if status.testnet != testnet {
                return Err(anyhow!(
                    "Server started but network mismatch. Stop it and retry with the correct --testnet flag."
                ));
            }
            let network = if testnet { "testnet" } else { "mainnet" };
            if output_opts.json {
                output::print_json_pretty(&status)?;
            } else {
                output::print_success(format!("Server started ({network})"));
            }
            return Ok(());
        }
    }

    let log_path = paths::server_log_path()?;
    Err(anyhow!(
        "Failed to start server. Check logs at: {}",
        log_path.display()
    ))
}

async fn server_stop(output_opts: OutputOptions) -> Result<()> {
    if !ServerClient::is_server_running()? {
        return Err(anyhow!(
            "Server is not running, run 'rhl server start' to start it"
        ));
    }

    // Try graceful shutdown via IPC.
    if let Ok(mut client) = ServerClient::connect().await {
        let _ = client.shutdown().await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Force kill via PID file (best effort).
    #[cfg(unix)]
    if ServerClient::is_server_running()?
        && let Ok(pid_path) = paths::server_pid_path()
        && let Ok(pid_str) = std::fs::read_to_string(&pid_path)
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
    }

    // Clean up files if still present.
    for p in [
        paths::server_socket_path().ok(),
        paths::server_pid_path().ok(),
        paths::server_config_path().ok(),
    ]
    .into_iter()
    .flatten()
    {
        let _ = std::fs::remove_file(p);
    }

    if output_opts.json {
        output::print_json_pretty(&serde_json::json!({ "stopped": true }))?;
    } else {
        output::print_success("Server stopped");
    }
    Ok(())
}

async fn server_status(output_opts: OutputOptions) -> Result<()> {
    if !ServerClient::is_server_running()? {
        if output_opts.json {
            output::print_json_pretty(&serde_json::json!({ "running": false }))?;
        } else {
            println!("Server is not running, run 'rhl server start' to start it");
        }
        return Ok(());
    }

    match ServerClient::connect().await {
        Ok(mut client) => match client.get_status().await {
            Ok(status) => {
                if output_opts.json {
                    output::print_json_pretty(&status)?;
                    return Ok(());
                }

                println!("Status:     running");
                println!(
                    "Network:    {}",
                    if status.testnet { "testnet" } else { "mainnet" }
                );
                println!(
                    "WebSocket:  {}",
                    if status.connected {
                        "connected"
                    } else {
                        "disconnected"
                    }
                );
                println!("Uptime:     {}", format_uptime(status.uptime));
                println!();
                println!("Cache:");
                println!(
                    "  Mid Prices:      {}",
                    if status.cache.has_mids {
                        format!("cached ({} ago)", format_age(status.cache.mids_age))
                    } else {
                        "not loaded".to_string()
                    }
                );
                println!(
                    "  Perp Meta:   {}",
                    if status.cache.has_perp_metas {
                        format!("cached ({} ago)", format_age(status.cache.perp_metas_age))
                    } else {
                        "not loaded".to_string()
                    }
                );
                println!(
                    "  Perp Asset Ctxs:  {}",
                    if status.cache.has_asset_ctxs {
                        format!("cached ({} ago)", format_age(status.cache.asset_ctxs_age))
                    } else {
                        "not loaded".to_string()
                    }
                );
                println!(
                    "  Spot Meta:   {}",
                    if status.cache.has_spot_meta {
                        format!("cached ({} ago)", format_age(status.cache.spot_meta_age))
                    } else {
                        "not loaded".to_string()
                    }
                );
                println!(
                    "  Spot Ctxs:   {}",
                    if status.cache.has_spot_asset_ctxs {
                        format!(
                            "cached ({} ago)",
                            format_age(status.cache.spot_asset_ctxs_age)
                        )
                    } else {
                        "not loaded".to_string()
                    }
                );
                Ok(())
            }
            Err(err) => {
                if output_opts.json {
                    output::print_json_pretty(
                        &serde_json::json!({ "running": true, "error": err.to_string() }),
                    )?;
                } else {
                    println!("Server appears running but not responding");
                    println!("Try: rhl server stop && rhl server start");
                }
                Ok(())
            }
        },
        Err(err) => {
            if output_opts.json {
                output::print_json_pretty(
                    &serde_json::json!({ "running": true, "error": err.to_string() }),
                )?;
            } else {
                println!("Server appears running but not responding");
                println!("Try: rhl server stop && rhl server start");
            }
            Ok(())
        }
    }
}

async fn run_upgrade(output_opts: OutputOptions) -> Result<()> {
    async fn fetch_latest_npm_version() -> Result<String> {
        let url = "https://registry.npmjs.org/hyperliquid-cli";
        let client = reqwest::Client::builder()
            .user_agent("hyperliquid-cli-rs")
            .build()?;
        let res = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !res.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch package info: HTTP {}",
                res.status()
            ));
        }
        let data = res
            .json::<serde_json::Value>()
            .await
            .context("Invalid JSON response")?;
        data.get("dist-tags")
            .and_then(|v| v.get("latest"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Invalid npm registry response: missing dist-tags.latest"))
    }

    fn is_update_available(current: &str, latest: &str) -> bool {
        let current_parts: Vec<u64> = current
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect();
        let latest_parts: Vec<u64> = latest
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect();

        for i in 0..current_parts.len().max(latest_parts.len()) {
            let current_part = *current_parts.get(i).unwrap_or(&0);
            let latest_part = *latest_parts.get(i).unwrap_or(&0);
            if latest_part > current_part {
                return true;
            }
            if latest_part < current_part {
                return false;
            }
        }
        false
    }

    let current = env!("CARGO_PKG_VERSION");
    let latest = match fetch_latest_npm_version().await {
        Ok(v) => v,
        Err(err) => {
            if output_opts.json {
                output::print_json_pretty(&serde_json::json!({ "error": err.to_string() }))?;
            } else {
                eprintln!("Failed to check for updates: {}", err);
            }
            std::process::exit(1);
        }
    };

    let update_available = is_update_available(current, &latest);

    if output_opts.json {
        output::print_json_pretty(&serde_json::json!({
            "current": current,
            "latest": latest,
            "updateAvailable": update_available,
        }))?;
        return Ok(());
    }

    println!("Current version: {current}");
    println!("Latest version:  {latest}");
    if update_available {
        println!();
        output::print_success("Update available!");
        println!();
        println!("To upgrade, run:");
        println!("  git pull");
        println!("  cargo install --path .");
    } else {
        println!();
        output::print_success("You are running the latest version.");
    }
    Ok(())
}
