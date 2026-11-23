#[derive(Debug, Clone, PartialEq)]
pub struct EditParams {
    /// Exposure in EV, linear multiplier 2^ev, range -3.0..3.0
    pub exposure: f32,
    /// S-curve strength around mid-gray 0.18, range -50..50
    pub contrast: i32,
    /// Highlight compression, range -100..100
    pub highlights: i32,
    /// Shadow lift, range -100..100
    pub shadows: i32,
    /// White balance temperature in Kelvin, 2000..10000
    pub temperature: u16,
    /// White balance tint, range -100..100
    pub tint: i32,
    /// True when parameters were computed by the auto-tone engine
    pub is_auto_applied: bool,
}

impl Default for EditParams {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            contrast: 0,
            highlights: 0,
            shadows: 0,
            temperature: 6500,
            tint: 0,
            is_auto_applied: false,
        }
    }
}

impl EditParams {
    pub fn is_neutral(&self) -> bool {
        self == &EditParams::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_neutral_and_within_ranges() {
        let params = EditParams::default();
        assert!(params.is_neutral());
        assert!((-3.0..=3.0).contains(&params.exposure));
        assert!((-50..=50).contains(&params.contrast));
        assert!((-100..=100).contains(&params.highlights));
        assert!((-100..=100).contains(&params.shadows));
        assert!((2000..=10000).contains(&params.temperature));
        assert!((-100..=100).contains(&params.tint));
        assert!(!params.is_auto_applied);
    }

    #[test]
    fn edited_params_are_not_neutral() {
        let params = EditParams {
            exposure: 0.5,
            ..Default::default()
        };
        assert!(!params.is_neutral());
    }
}
