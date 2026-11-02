//! Member characters: the look of a member's avatar, from the jojo-friends family
//! (docs/protocol.md, `Member.character`). Unknown ids fail to parse.

use serde::{Deserialize, Serialize};

macro_rules! characters {
    ($($variant:ident = $id:literal),* $(,)?) => {
        /// One of the 26 known jojo-friends character ids.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum CharacterId {
            $(#[serde(rename = $id)] $variant,)*
        }

        impl CharacterId {
            /// Every id, in catalogue order.
            pub const ALL: &'static [CharacterId] = &[$(CharacterId::$variant,)*];

            /// The wire id, for example `"jojo"` or `"o03"`.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(CharacterId::$variant => $id,)*
                }
            }

            /// Parses a wire id; `None` for an unknown id.
            pub fn parse(id: &str) -> Option<CharacterId> {
                match id {
                    $($id => Some(CharacterId::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

characters! {
    Jojo = "jojo",
    H02 = "h02", H03 = "h03", H04 = "h04", H05 = "h05", H06 = "h06",
    O01 = "o01", O02 = "o02", O03 = "o03", O04 = "o04", O05 = "o05",
    O06 = "o06", O07 = "o07", O08 = "o08", O09 = "o09", O10 = "o10",
    O11 = "o11", O12 = "o12", O13 = "o13", O14 = "o14", O15 = "o15",
    O16 = "o16", O17 = "o17", O18 = "o18", O19 = "o19", O20 = "o20",
}

impl std::fmt::Display for CharacterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::CharacterId;

    #[test]
    fn ids_round_trip() {
        assert_eq!(CharacterId::ALL.len(), 26);
        for id in CharacterId::ALL {
            assert_eq!(CharacterId::parse(id.as_str()), Some(*id));
            let json = serde_json::to_string(id).unwrap();
            assert_eq!(json, format!("\"{}\"", id.as_str()));
        }
        assert_eq!(CharacterId::parse("o21"), None);
    }
}
