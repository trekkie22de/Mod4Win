//! Skin-Modus (`SkinMode`) und Live-Theme-Switch (`F11` / Setup / CLI).

/// Wählbare UI-Skins (im Setup-Dialog oder per `--skin <2.40|modern>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SkinMode {
    /// 1. `2.40 Classic` (`0`): MOD4WIN 2.40 Classic (Windows 95 Silber-Blau-Titelleiste + 3D-Grau)
    #[default]
    Classic240 = 0,
    /// 2. `Modern` (`3`): Modern Skin (MOD4WIN 3.00)
    Modern = 3,
}

impl SkinMode {
    pub fn from_u8(val: u8) -> Self {
        match val {
            2 | 3 => Self::Modern,
            _ => Self::Classic240,
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Modern => 3,
            Self::Classic240 => 0,
        }
    }

    pub fn is_modern(self) -> bool {
        matches!(self, Self::Modern)
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        let norm = s.trim().to_ascii_lowercase();
        if norm.contains("2.40")
            || norm.contains("240")
            || norm == "win95"
            || norm == "classic240"
            || norm == "classic"
            || norm == "default"
        {
            Some(Self::Classic240)
        } else if norm == "modern" || norm == "modn" || norm == "rack" {
            Some(Self::Modern)
        } else {
            None
        }
    }

    /// Schaltet zyklisch zwischen `Classic (2.40)` und `Modern` um.
    pub fn next(self) -> Self {
        match self {
            Self::Classic240 => Self::Modern,
            Self::Modern => Self::Classic240,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Classic240 => "2.40 Classic",
            Self::Modern => "Modern",
        }
    }

    pub fn short_setup_label(self) -> &'static str {
        match self {
            Self::Classic240 => "2.40",
            Self::Modern => "Modern",
        }
    }

    pub fn lcd_banner(self) -> &'static str {
        match self {
            Self::Classic240 => "SKIN: 2.40 CLASSIC",
            Self::Modern => "SKIN: MODERN",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skin_mode_cycle_and_parsing() {
        assert_eq!(SkinMode::parse_str("2.40"), Some(SkinMode::Classic240));
        assert_eq!(SkinMode::parse_str("classic"), Some(SkinMode::Classic240));
        assert_eq!(SkinMode::parse_str("modern"), Some(SkinMode::Modern));

        let s0 = SkinMode::Classic240;
        let s1 = s0.next();
        let s2 = s1.next();
        assert_eq!(s1, SkinMode::Modern);
        assert_eq!(s2, SkinMode::Classic240);
    }
}
