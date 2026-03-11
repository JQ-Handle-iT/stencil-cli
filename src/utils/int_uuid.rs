use anyhow::{anyhow, Result};
use regex::Regex;

pub const UUID_REGEXP: &str = r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-([0-9a-f]{12})";

/// Convert a number to the fake UUID format used by BigCommerce locally
pub fn int2uuid(n: u64) -> String {
    format!("00000000-0000-0000-0000-{:012}", n)
}

/// Convert a fake UUID back to an integer
pub fn uuid2int(uuid: &str) -> Result<u64> {
    let re = Regex::new(UUID_REGEXP)?;
    let caps = re
        .captures(uuid)
        .ok_or_else(|| anyhow!("Not a uuid match for {}", uuid))?;
    let num_str = caps.get(1).unwrap().as_str();
    Ok(num_str.parse::<u64>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int2uuid() {
        assert_eq!(int2uuid(1), "00000000-0000-0000-0000-000000000001");
        assert_eq!(int2uuid(42), "00000000-0000-0000-0000-000000000042");
    }

    #[test]
    fn test_uuid2int() {
        assert_eq!(uuid2int("00000000-0000-0000-0000-000000000001").unwrap(), 1);
        assert_eq!(uuid2int("00000000-0000-0000-0000-000000000042").unwrap(), 42);
    }
}
