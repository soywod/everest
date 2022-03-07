use std::{
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EverestError {
    #[error("cannot find uid on imap message {0}")]
    MissingImapUidError(u32),
    #[error("cannot get maildir entry: {0}")]
    InvalidMaildirEntryError(String),
}

#[derive(PartialEq, Eq, Hash)]
enum Flag {
    Draft,
    Flagged,
    Replied,
    Seen,
    Trashed,
}

#[derive(Default)]
struct Flags(HashSet<Flag>);

impl Deref for Flags {
    type Target = HashSet<Flag>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Flags {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

struct Envelope {
    id: String,
    flags: Flags,
}

#[derive(Default)]
struct Envelopes(HashMap<String, Envelope>);

impl Deref for Envelopes {
    type Target = HashMap<String, Envelope>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Envelopes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<imap::types::Fetches> for Envelopes {
    type Error = EverestError;

    fn try_from(fetches: imap::types::Fetches) -> Result<Self, Self::Error> {
        let mut envelopes = Envelopes::default();
        for fetch in fetches.iter() {
            let id = fetch
                .uid
                .ok_or_else(|| EverestError::MissingImapUidError(fetch.message))?
                .to_string();
            let flags = fetch
                .flags()
                .iter()
                .fold(Flags::default(), |mut flags, flag| {
                    match flag {
                        imap::types::Flag::Seen => flags.insert(Flag::Seen),
                        imap::types::Flag::Answered => flags.insert(Flag::Replied),
                        imap::types::Flag::Flagged => flags.insert(Flag::Flagged),
                        imap::types::Flag::Deleted => flags.insert(Flag::Trashed),
                        imap::types::Flag::Draft => flags.insert(Flag::Draft),
                        _ => false,
                    };
                    flags
                });
            envelopes.insert(id.clone(), Envelope { id, flags });
        }
        Ok(envelopes)
    }
}

impl TryFrom<maildir::MailEntries> for Envelopes {
    type Error = EverestError;

    fn try_from(entries: maildir::MailEntries) -> Result<Self, Self::Error> {
        let mut envelopes = Envelopes::default();
        for entry in entries {
            let entry = entry.map_err(|e| EverestError::InvalidMaildirEntryError(e.to_string()))?;
            let id = entry.id().to_owned();
            let flags = entry
                .flags()
                .chars()
                .fold(Flags::default(), |mut flags, c| {
                    match c {
                        'S' => flags.insert(Flag::Seen),
                        'R' => flags.insert(Flag::Replied),
                        'F' => flags.insert(Flag::Flagged),
                        'T' => flags.insert(Flag::Trashed),
                        'D' => flags.insert(Flag::Draft),
                        _ => false,
                    };
                    flags
                });
            envelopes.insert(id.clone(), Envelope { id, flags });
        }
        Ok(envelopes)
    }
}
