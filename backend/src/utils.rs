use chrono::{Datelike, NaiveDate};
use sha2::{Digest, Sha256};

/// Returns the Russian week parity used by the schedule feed.
pub fn week_parity(date: NaiveDate) -> &'static str {
    if date.iso_week().week() % 2 == 0 {
        "чёт"
    } else {
        "нечёт"
    }
}

/// Stable lowercase hexadecimal SHA-256 for cache keys and change detection.
pub fn content_hash(input: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(input.as_ref()))
}

/// Extracts a numeric building identifier from room labels such as
/// "ауд. 2.207" or "корпус 3, ауд. 3.045".
pub fn extract_building(room: &str) -> Option<String> {
    let lower = room.to_lowercase();
    for marker in ["ауд.", "ауд ", "корпус", "корп."] {
        if let Some(pos) = lower.find(marker) {
            let tail = room[pos + marker.len()..].trim_start_matches([' ', '.', ':']);
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return Some(digits);
            }
        }
    }
    // Common compact room notation: 2.207.
    let trimmed = room.trim();
    let mut parts = trimmed.split('.');
    let first = parts.next()?;
    let second = parts.next()?;
    if !first.is_empty()
        && first.chars().all(|c| c.is_ascii_digit())
        && second.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return Some(first.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parity_uses_iso_week_number() {
        assert_eq!(
            week_parity(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()),
            "чёт"
        );
        assert_eq!(
            week_parity(NaiveDate::from_ymd_opt(2026, 1, 12).unwrap()),
            "нечёт"
        );
    }
    #[test]
    fn extracts_building_from_room() {
        assert_eq!(extract_building("ауд. 2.207"), Some("2".into()));
        assert_eq!(extract_building("корпус 3, ауд. 3.045"), Some("3".into()));
        assert_eq!(extract_building("Открытый стадион"), None);
    }
    #[test]
    fn hash_is_stable_sha256() {
        assert_eq!(
            content_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
