<h1 align="center">
     Orca SDK
</h1>
<h4 align="center">
一个功能完整的 Orca SDK，用于与 Solana 上的 Orca DEX 协议进行交互。支持集中流动性池、标准池和稳定池的交易、流动性提供和价格监控。
</h4>
<p align="center">
  <a href="https://github.com/0xhappyboy/orca-sdk/LICENSE"><img src="https://img.shields.io/badge/License-GPL3.0-d1d1f6.svg?style=flat&labelColor=1C2C2E&color=BEC5C9&logo=googledocs&label=license&logoColor=BEC5C9" alt="License"></a>
</p>
<p align="center">
<a href="./README_zh-CN.md">简体中文</a> | <a href="./README.md">English</a>
</p>

## 功能特性

- 🏊 完整的 Orca 协议支持 - Whirlpools（集中流动性）、标准池、稳定池
- 💰 代币余额管理 - 查询余额、创建代币账户
- 🔄 交易功能 - 代币兑换、滑点保护
- 💧 流动性管理 - 添加/移除流动性、仓位管理
- 📊 价格数据 - 实时价格、K 线数据、价格历史
- 🚨 监控功能 - 价格变化监控、池子健康度检查
- 🔍 链上数据分析 - 交易分析、池子发现

## 案例

### 初始化客户端

```rust
use orca_rs::OrcaClient;
use solana_sdk::signature::Keypair;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建客户端
    let client = OrcaClient::new()?;
    // 或者使用 Arc 包装用于并发
    let client = std::sync::Arc::new(client);
    Ok(())
}
```

### 查询代币余额

```rust
use solana_sdk::pubkey;

async fn check_balances(client: &OrcaClient) -> Result<(), Box<dyn std::error::Error>> {
    let owner = pubkey!("YourWalletPublicKeyHere");
    let mint = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"); // USDC
    // 查询特定代币余额
    let balance = client.get_token_balance(&owner, &mint).await?;
    println!("USDC 余额: {}", balance);
    // 查询所有代币余额
    let all_balances = client.get_all_token_balances(&owner).await?;
    for (mint, balance) in all_balances {
        println!("代币: {}, 余额: {}", mint, balance);
    }
    Ok(())
}
```

### 执行交易

```rust
use orca_rs::trade::TradeConfig;

async fn execute_swap(client: &OrcaClient, keypair: &Keypair) -> Result<(), Box<dyn std::error::Error>> {
    let input_mint = "So11111111111111111111111111111111111111112"; // SOL
    let output_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"; // USDC
    let amount = 1_000_000; // 1 SOL 
    let config = TradeConfig {
        slippage: 0.5, // 0.5% 滑点容忍度
        max_iterations: 3,
    };
    let signature = client.swap(keypair, input_mint, output_mint, amount, Some(config)).await?;
    println!("交易完成! 签名: {}", signature);
    Ok(())
}
```

### 提供流动性

```rust
use orca_rs::liquidity::{LiquidityPosition, AddLiquidityConfig};

async fn add_liquidity(client: &OrcaClient, keypair: &Keypair) -> Result<(), Box<dyn std::error::Error>> {
    let pool_address = "whirlpool_address_here";
    let pool_info = client.get_pool_state_onchain(pool_address).await?;
    let token_a_amount = 1_000_000; // 代币 A 数量
    let token_b_amount = 2_000_000; // 代币 B 数量
    let lower_tick = -1000; // 价格下限
    let upper_tick = 1000;  // 价格上限
    let config = AddLiquidityConfig {
        slippage_tolerance: 0.5,
        max_iterations: 3,
    };
    let signature = client.add_liquidity(
        keypair,
        &pool_info,
        token_a_amount,
        token_b_amount,
        lower_tick,
        upper_tick,
        Some(config),
    ).await?;
    println!("流动性添加成功! 交易签名: {}", signature);
    Ok(())
}

async fn check_positions(client: &OrcaClient, owner: &Pubkey) -> Result<(), Box<dyn std::error::Error>> {
    let positions = client.get_liquidity_positions(owner).await?;
    for position in positions {
        println!("流动性仓位: {} LP 代币", position.lp_token_amount);
        println!("代币 A: {}, 代币 B: {}", position.token_a_amount, position.token_b_amount);
        println!("价格区间: {} 到 {}", position.lower_tick, position.upper_tick);
    }
    Ok(())
}
```

### 价格监控

```rust
use orca_rs::events::PriceUpdate;
use std::sync::Arc;

async fn monitor_prices(client: Arc<OrcaClient>) -> Result<(), Box<dyn std::error::Error>> {
    let pool_address = "whirlpool_address_here";

    let monitor_handle = client.monitor_price_changes_production(
        pool_address,
        1.0, // 1% 价格变化阈值
        |update: PriceUpdate| {
            println!("检测到价格变化!");
            println!("池子: {}", update.pool_address);
            println!("旧价格: {}, 新价格: {}", update.old_price, update.new_price);
            println!("变化: {:.2}%", update.change_percent);
            println!("时间: {}", update.timestamp);
        },
    ).await?;
    // 运行监控 60 秒
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    // 关闭
    monitor_handle.shutdown().await;
    Ok(())
}
```

### 获取价格数据

```rust
async fn get_price_data(client: &OrcaClient) -> Result<(), Box<dyn std::error::Error>> {
    let base_mint = "So11111111111111111111111111111111111111112"; // SOL
    let quote_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"; // USDC
    // 获取当前价格
    let current_price = client.get_token_price_from_pool(base_mint, quote_mint).await?;
    println!("当前 SOL/USDC 价格: {}", current_price);
    // 获取价格历史
    let pool_address = "whirlpool_sol_usdc_address";
    let price_history = client.get_price_history_from_chain(pool_address, 100).await?;
    for data in price_history {
        println!("时间: {}, 价格: {}", data.timestamp, data.price);
    }
    // 计算移动平均
    let ma_20 = client.calculate_moving_average_from_chain(pool_address, 20).await?;
    println!("20周期移动平均: {}", ma_20);
    // 获取 K 线数据
    let klines = client.get_kline_data_production(pool_address, 60, 100).await?; // 1小时K线
    for kline in klines {
        println!("开盘: {}, 最高: {}, 最低: {}, 收盘: {}",
                 kline.open, kline.high, kline.low, kline.close);
    }
    Ok(())
}
```

### 池子健康度检查

```rust
async fn check_pool_health(client: &OrcaClient) -> Result<(), Box<dyn std::error::Error>> {
    let pool_address = "whirlpool_address_here";

    let health = client.monitor_pool_health(pool_address).await?;

    println!("池子健康度报告:");
    println!("流动性: {}", health.liquidity);
    println!("24小时交易量: {}", health.volume_24h);
    println!("手续费增长: {}", health.fee_growth);
    println!("健康度评分: {:.2}", health.health_score);

    if health.health_score > 80.0 {
        println!("✅ 池子健康状态良好");
    } else if health.health_score > 50.0 {
        println!("⚠️  池子健康状态一般");
    } else {
        println!("❌ 池子健康状态较差");
    }

    Ok(())
}
```
