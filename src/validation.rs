use std::cmp::Ordering;

use anyhow::{Result, anyhow};
use ethers::types::Address;
use hyperliquid::types::exchange::request::Tif;

pub fn validate_address(value: &str) -> Result<Address> {
    if !value.starts_with("0x") || value.len() != 42 {
        return Err(anyhow!("Invalid address: {value}"));
    }
    value
        .parse::<Address>()
        .map_err(|_| anyhow!("Invalid address: {value}"))
}

pub fn validate_private_key(value: &str) -> Result<String> {
    let trimmed = value.trim();
    let (maybe_prefix, hex) = if let Some(rest) = trimmed.strip_prefix("0x") {
        ("0x", rest)
    } else if let Some(rest) = trimmed.strip_prefix("0X") {
        ("0x", rest)
    } else {
        ("0x", trimmed)
    };

    let is_hex_64 = hex.len() == 64 && hex.as_bytes().iter().all(|b| b.is_ascii_hexdigit());
    if !is_hex_64 {
        return Err(anyhow!(
            "Invalid private key format. Must be a 64-character hex string."
        ));
    }

    Ok(format!("{maybe_prefix}{hex}"))
}

pub fn validate_positive_number(value: &str, name: &str) -> Result<f64> {
    let num: f64 = value
        .parse()
        .map_err(|_| anyhow!("{name} must be a positive number"))?;
    if num.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return Err(anyhow!("{name} must be a positive number"));
    }
    Ok(num)
}

pub fn validate_non_negative_number(value: &str, name: &str) -> Result<f64> {
    let num: f64 = value
        .parse()
        .map_err(|_| anyhow!("{name} must be a non-negative number"))?;
    match num.partial_cmp(&0.0) {
        Some(Ordering::Less) | None => {
            return Err(anyhow!("{name} must be a non-negative number"));
        }
        Some(Ordering::Equal | Ordering::Greater) => {}
    }
    Ok(num)
}

pub fn validate_positive_u64(value: &str, name: &str) -> Result<u64> {
    let num: f64 = value
        .parse::<f64>()
        .map_err(|_| anyhow!("{name} must be a positive integer"))?;
    if !num.is_finite() {
        return Err(anyhow!("{name} must be a positive integer"));
    }
    let truncated = num.trunc();
    if truncated <= 0.0 {
        return Err(anyhow!("{name} must be a positive integer"));
    }
    Ok(truncated as u64)
}

pub fn validate_side(value: &str) -> Result<String> {
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "buy" | "sell" => Ok(lower),
        _ => Err(anyhow!("Side must be \"buy\" or \"sell\"")),
    }
}

pub fn validate_side_with_aliases(value: &str) -> Result<String> {
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "long" | "buy" => Ok("buy".to_string()),
        "short" | "sell" => Ok("sell".to_string()),
        _ => Err(anyhow!(
            "Side must be \"buy\", \"sell\", \"long\", or \"short\""
        )),
    }
}

pub fn validate_tif(value: &str) -> Result<Tif> {
    match value.to_ascii_lowercase().as_str() {
        "gtc" => Ok(Tif::Gtc),
        "ioc" => Ok(Tif::Ioc),
        "alo" => Ok(Tif::Alo),
        _ => Err(anyhow!(
            "Time-in-force must be \"Gtc\", \"Ioc\", or \"Alo\""
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_address() {
        let addr = "0x1234567890abcdef1234567890abcdef12345678";
        assert_eq!(
            validate_address(addr).unwrap(),
            addr.parse::<Address>().unwrap()
        );

        let upper = "0x1234567890ABCDEF1234567890ABCDEF12345678";
        assert_eq!(
            validate_address(upper).unwrap(),
            upper.parse::<Address>().unwrap()
        );

        assert!(
            validate_address("1234567890abcdef1234567890abcdef12345678")
                .unwrap_err()
                .to_string()
                .contains("Invalid address")
        );
        assert!(
            validate_address("0x1234567890abcdef")
                .unwrap_err()
                .to_string()
                .contains("Invalid address")
        );
        assert!(
            validate_address("0x1234567890abcdef1234567890abcdef1234567g")
                .unwrap_err()
                .to_string()
                .contains("Invalid address")
        );
    }

    #[test]
    fn validates_private_key() {
        let pk = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        assert_eq!(validate_private_key(pk).unwrap(), pk);

        let no_prefix = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        assert_eq!(
            validate_private_key(no_prefix).unwrap(),
            format!("0x{no_prefix}")
        );

        let upper = "0x1234567890ABCDEF1234567890ABCDEF1234567890ABCDEF1234567890ABCDEF";
        assert_eq!(validate_private_key(upper).unwrap(), upper);

        assert!(validate_private_key("0x1234567890abcdef").is_err());
    }

    #[test]
    fn validates_positive_number() {
        assert_eq!(validate_positive_number("42", "value").unwrap(), 42.0);
        assert_eq!(validate_positive_number("3.14", "price").unwrap(), 3.14);
        assert_eq!(validate_positive_number("0.0001", "size").unwrap(), 0.0001);

        assert!(validate_positive_number("0", "amount").is_err());
        assert!(validate_positive_number("-5", "price").is_err());
        assert!(validate_positive_number("abc", "size").is_err());
        assert!(validate_positive_number("", "value").is_err());
    }

    #[test]
    fn validates_positive_integer() {
        assert_eq!(validate_positive_u64("42", "count").unwrap(), 42);
        assert_eq!(validate_positive_u64("3.9", "leverage").unwrap(), 3);

        assert!(validate_positive_u64("0", "leverage").is_err());
        assert!(validate_positive_u64("-5", "leverage").is_err());
        assert!(validate_positive_u64("abc", "count").is_err());
    }

    #[test]
    fn validates_side() {
        assert_eq!(validate_side("buy").unwrap(), "buy");
        assert_eq!(validate_side("sell").unwrap(), "sell");
        assert_eq!(validate_side("BUY").unwrap(), "buy");
        assert_eq!(validate_side("SELL").unwrap(), "sell");
        assert_eq!(validate_side("Buy").unwrap(), "buy");

        assert!(validate_side("long").is_err());
        assert!(validate_side("").is_err());
    }

    #[test]
    fn validates_side_aliases() {
        assert_eq!(validate_side_with_aliases("long").unwrap(), "buy");
        assert_eq!(validate_side_with_aliases("short").unwrap(), "sell");
        assert!(validate_side_with_aliases("nope").is_err());
    }

    #[test]
    fn validates_tif_case_insensitive() {
        assert!(matches!(validate_tif("gtc").unwrap(), Tif::Gtc));
        assert!(matches!(validate_tif("ioc").unwrap(), Tif::Ioc));
        assert!(matches!(validate_tif("alo").unwrap(), Tif::Alo));
        assert!(matches!(validate_tif("GTC").unwrap(), Tif::Gtc));
        assert!(matches!(validate_tif("Ioc").unwrap(), Tif::Ioc));

        assert!(validate_tif("fok").is_err());
        assert!(validate_tif("").is_err());
    }
}
