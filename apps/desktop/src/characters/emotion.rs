//! The emotion a character shows, and the [`Look`] (status and emotion) an avatar draws.
//!
//! Status and emotion are two channels (Jojo & Friends, docs/SPEC.md §5): the status says
//! what the bot does and never changes the face; the emotion is the face. Until a bot sets
//! its own mood, the emotion follows from what the status tracker knows
//! ([`super::status::look_of`]).

use super::CharacterStatus;

/// The 16 emotions of the avatar pack. The wire and file names are the pack's ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Emotion {
    #[default]
    Calm,
    Focus,
    Think,
    Puzzled,
    Curious,
    Happy,
    Laugh,
    Smug,
    Shy,
    Celebrate,
    Surprised,
    Angry,
    Sleepy,
    Aggrieved,
    Cry,
    Sob,
}

impl Emotion {
    #[cfg(test)]
    pub const ALL: [Emotion; 16] = [
        Emotion::Calm,
        Emotion::Focus,
        Emotion::Think,
        Emotion::Puzzled,
        Emotion::Curious,
        Emotion::Happy,
        Emotion::Laugh,
        Emotion::Smug,
        Emotion::Shy,
        Emotion::Celebrate,
        Emotion::Surprised,
        Emotion::Angry,
        Emotion::Sleepy,
        Emotion::Aggrieved,
        Emotion::Cry,
        Emotion::Sob,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Emotion::Calm => "calm",
            Emotion::Focus => "focus",
            Emotion::Think => "think",
            Emotion::Puzzled => "puzzled",
            Emotion::Curious => "curious",
            Emotion::Happy => "happy",
            Emotion::Laugh => "laugh",
            Emotion::Smug => "smug",
            Emotion::Shy => "shy",
            Emotion::Celebrate => "celebrate",
            Emotion::Surprised => "surprised",
            Emotion::Angry => "angry",
            Emotion::Sleepy => "sleepy",
            Emotion::Aggrieved => "aggrieved",
            Emotion::Cry => "cry",
            Emotion::Sob => "sob",
        }
    }

    /// The emotion a status shows when nothing more is known: calm, focus, curious, happy,
    /// puzzled, sleepy. An error never makes a character cry.
    pub fn of_status(status: CharacterStatus) -> Emotion {
        match status {
            CharacterStatus::Idle => Emotion::Calm,
            CharacterStatus::Working => Emotion::Focus,
            CharacterStatus::NeedsInput => Emotion::Curious,
            CharacterStatus::Success => Emotion::Happy,
            CharacterStatus::Error => Emotion::Puzzled,
            CharacterStatus::Offline => Emotion::Sleepy,
        }
    }
}

/// What a character avatar draws: its status and its face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Look {
    pub status: CharacterStatus,
    pub emotion: Emotion,
}

impl Look {
    /// The pack file the face comes from: the emotion, or `offline` (sleepy and dimmed),
    /// which is its own picture.
    pub fn face(self) -> &'static str {
        match self.status {
            CharacterStatus::Offline => "offline",
            _ => self.emotion.as_str(),
        }
    }
}

impl From<CharacterStatus> for Look {
    fn from(status: CharacterStatus) -> Self {
        Look {
            status,
            emotion: Emotion::of_status(status),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_alone_gives_its_default_face() {
        use CharacterStatus::*;
        let faces: Vec<_> = CharacterStatus::ALL
            .iter()
            .map(|&s| Look::from(s).face())
            .collect();
        assert_eq!(
            faces,
            ["calm", "focus", "curious", "happy", "puzzled", "offline"]
        );
        // Offline is its own picture whatever the emotion.
        let look = Look {
            status: Offline,
            emotion: Emotion::Laugh,
        };
        assert_eq!(look.face(), "offline");
        assert_eq!(
            Look {
                status: Working,
                emotion: Emotion::Think
            }
            .face(),
            "think"
        );
    }

    #[test]
    fn the_ids_are_distinct() {
        let ids: std::collections::HashSet<_> = Emotion::ALL.iter().map(|e| e.as_str()).collect();
        assert_eq!(ids.len(), 16);
    }
}
