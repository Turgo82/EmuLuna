//! Explicit deterministic diagnostic input. Ordinary playback has no script.
#[derive(Debug, Default)]
pub struct InputScript(Vec<(u32, u32, u32)>);
impl InputScript {
    /// Inclusive frame ranges, e.g. "60-62:8,120:1". Masks use host button bits.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut steps = Vec::new();
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        for entry in text.split(',') {
            if steps.len() >= 512 {
                return Err("Input script has too many steps".into());
            }
            let (range, mask) = entry
                .trim()
                .split_once(':')
                .ok_or("Expected frame[-frame]:mask")?;
            let mask: u32 = mask.parse().map_err(|_| "Invalid input mask")?;
            if mask > 65535 {
                return Err("Input mask exceeds the standard controller bits".into());
            }
            let (start, end) = range.split_once('-').unwrap_or((range, range));
            let start: u32 = start.parse().map_err(|_| "Invalid start frame")?;
            let end: u32 = end.parse().map_err(|_| "Invalid end frame")?;
            if start > end || end >= 36000 {
                return Err("Input range must be ordered within 0..36000".into());
            }
            steps.push((start, end, mask));
        }
        Ok(Self(steps))
    }
    pub fn buttons(&self, frame: u32) -> u32 {
        self.0
            .iter()
            .filter(|(start, end, _)| (*start..=*end).contains(&frame))
            .fold(0, |bits, (_, _, mask)| bits | mask)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presses_release_and_overlapping_buttons_combine() {
        let script = InputScript::parse("60-62:8,61:1").unwrap();
        assert_eq!(script.buttons(59), 0);
        assert_eq!(script.buttons(60), 8);
        assert_eq!(script.buttons(61), 9);
        assert_eq!(script.buttons(62), 8);
        assert_eq!(script.buttons(63), 0);
    }
    #[test]
    fn malformed_scripts_fail_before_execution() {
        for text in ["1-0:1", "0:65536", "36000:1", "start:1", "0", "1:bad"] {
            assert!(InputScript::parse(text).is_err(), "{text}");
        }
    }
}
