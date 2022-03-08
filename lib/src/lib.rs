use std::{
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EverestError {
    #[error("cannot find uid on imap message {0}")]
    MissingImapUidError(u32),
    #[error("cannot get maildir entry: {0}")]
    InvalidMaildirEntryError(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Flag {
    Draft,
    Flagged,
    Replied,
    Seen,
    Trashed,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
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

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct Envelope {
    id: String,
    flags: Flags,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Hunk {
    Imap(HunkKind),
    Maildir(HunkKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HunkKind {
    AddMsg(String),
    RemoveMsg(String),
    AddFlag(String, Flag),
    RemoveFlag(String, Flag),
}

type Patch = Vec<Hunk>;

fn build_patch(
    prev_imap_envelopes: Envelopes,
    next_imap_envelopes: Envelopes,
    prev_mdir_envelopes: Envelopes,
    next_mdir_envelopes: Envelopes,
) -> Patch {
    let mut ids = HashSet::new();
    ids.extend(next_imap_envelopes.iter().map(|(id, _)| id.as_str()));
    ids.extend(prev_imap_envelopes.iter().map(|(id, _)| id.as_str()));
    ids.extend(next_mdir_envelopes.iter().map(|(id, _)| id.as_str()));
    ids.extend(prev_mdir_envelopes.iter().map(|(id, _)| id.as_str()));

    let mut patch = vec![];

    for id in ids {
        // id present only in imap
        if next_imap_envelopes.contains_key(id)
            && !prev_imap_envelopes.contains_key(id)
            && !next_mdir_envelopes.contains_key(id)
            && !prev_mdir_envelopes.contains_key(id)
        {
            // add maildir msg
            patch.push(Hunk::Maildir(HunkKind::AddMsg(id.to_owned())))
        }

        // id present only in maildir
        if !next_imap_envelopes.contains_key(id)
            && !prev_imap_envelopes.contains_key(id)
            && next_mdir_envelopes.contains_key(id)
            && !prev_mdir_envelopes.contains_key(id)
        {
            // add imap msg
            patch.push(Hunk::Imap(HunkKind::AddMsg(id.to_owned())))
        }

        // id everywhere except in imap
        if !next_imap_envelopes.contains_key(id)
            && prev_imap_envelopes.contains_key(id)
            && next_mdir_envelopes.contains_key(id)
            && prev_mdir_envelopes.contains_key(id)
        {
            // remove maildir msg
            patch.push(Hunk::Maildir(HunkKind::RemoveMsg(id.to_owned())))
        }

        // id everywhere except in maildir
        if next_imap_envelopes.contains_key(id)
            && prev_imap_envelopes.contains_key(id)
            && !next_mdir_envelopes.contains_key(id)
            && prev_mdir_envelopes.contains_key(id)
        {
            // remove imap msg
            patch.push(Hunk::Imap(HunkKind::RemoveMsg(id.to_owned())))
        }

        // id everywhere
        if next_imap_envelopes.contains_key(id)
            && prev_imap_envelopes.contains_key(id)
            && next_mdir_envelopes.contains_key(id)
            && prev_mdir_envelopes.contains_key(id)
        {
            let imap_envelope = next_imap_envelopes.get(id).unwrap();
            let imap_cache_envelope = prev_imap_envelopes.get(id).unwrap();
            let mdir_envelope = next_mdir_envelopes.get(id).unwrap();
            let mdir_cache_envelope = prev_mdir_envelopes.get(id).unwrap();

            for ref flag in [
                Flag::Draft,
                Flag::Flagged,
                Flag::Replied,
                Flag::Seen,
                Flag::Trashed,
            ] {
                // flag in imap but not in imap cache
                if imap_envelope.flags.contains(flag) && !imap_cache_envelope.flags.contains(flag) {
                    // add maildir flag
                    patch.push(Hunk::Maildir(HunkKind::AddFlag(
                        id.to_owned(),
                        flag.to_owned(),
                    )))
                }

                // flag not in imap but in imap cache
                if !imap_envelope.flags.contains(flag) && imap_cache_envelope.flags.contains(flag) {
                    // remove maildir flag
                    patch.push(Hunk::Maildir(HunkKind::RemoveFlag(
                        id.to_owned(),
                        flag.to_owned(),
                    )))
                }

                // flag present only in maildir
                if !imap_envelope.flags.contains(flag)
                    && !imap_cache_envelope.flags.contains(flag)
                    && mdir_envelope.flags.contains(flag)
                    && !mdir_cache_envelope.flags.contains(flag)
                {
                    // add imap flag
                    patch.push(Hunk::Imap(HunkKind::AddFlag(
                        id.to_owned(),
                        flag.to_owned(),
                    )))
                }

                // flag everywhere except in maildir
                if imap_envelope.flags.contains(flag)
                    && imap_cache_envelope.flags.contains(flag)
                    && !mdir_envelope.flags.contains(flag)
                    && mdir_cache_envelope.flags.contains(flag)
                {
                    // remove imap flag
                    patch.push(Hunk::Imap(HunkKind::RemoveFlag(
                        id.to_owned(),
                        flag.to_owned(),
                    )))
                }
            }
        }
    }

    patch
}

#[cfg(test)]
mod tests {
    use std::iter::FromIterator;

    use super::*;

    #[test]
    fn add_imap_msg_test() {
        let env1 = Envelope {
            id: "1".into(),
            flags: Flags(HashSet::from_iter([Flag::Seen])),
        };
        let env2 = Envelope {
            id: "2".into(),
            flags: Flags(HashSet::from_iter([Flag::Flagged])),
        };

        let prev_imap_envelopes = Envelopes(HashMap::from_iter([(env1.id.clone(), env1.clone())]));
        let next_imap_envelopes = Envelopes(HashMap::from_iter([(env1.id.clone(), env1.clone())]));
        let prev_mdir_envelopes = Envelopes(HashMap::from_iter([(env1.id.clone(), env1.clone())]));
        let next_mdir_envelopes = Envelopes(HashMap::from_iter([
            (env1.id.clone(), env1.clone()),
            (env2.id.clone(), env2.clone()),
        ]));

        let patch = build_patch(
            prev_imap_envelopes,
            next_imap_envelopes,
            prev_mdir_envelopes,
            next_mdir_envelopes,
        );

        assert_eq!(vec![Hunk::Imap(HunkKind::AddMsg("2".into()))], patch);
    }

    #[test]
    fn remove_imap_msg_test() {
        let env1 = Envelope {
            id: "1".into(),
            flags: Flags(HashSet::from_iter([Flag::Seen])),
        };
        let env2 = Envelope {
            id: "2".into(),
            flags: Flags(HashSet::from_iter([Flag::Flagged])),
        };

        let prev_imap_envelopes = Envelopes(HashMap::from_iter([
            (env1.id.clone(), env1.clone()),
            (env2.id.clone(), env2.clone()),
        ]));
        let next_imap_envelopes = Envelopes(HashMap::from_iter([
            (env1.id.clone(), env1.clone()),
            (env2.id.clone(), env2.clone()),
        ]));
        let prev_mdir_envelopes = Envelopes(HashMap::from_iter([
            (env1.id.clone(), env1.clone()),
            (env2.id.clone(), env2.clone()),
        ]));
        let next_mdir_envelopes = Envelopes(HashMap::from_iter([(env1.id.clone(), env1.clone())]));

        let patch = build_patch(
            prev_imap_envelopes,
            next_imap_envelopes,
            prev_mdir_envelopes,
            next_mdir_envelopes,
        );

        assert_eq!(vec![Hunk::Imap(HunkKind::RemoveMsg("2".into()))], patch);
    }

    #[test]
    fn add_mdir_msg_test() {
        let env1 = Envelope {
            id: "1".into(),
            flags: Flags(HashSet::from_iter([Flag::Seen])),
        };
        let env2 = Envelope {
            id: "2".into(),
            flags: Flags(HashSet::from_iter([Flag::Flagged])),
        };

        let prev_imap_envelopes = Envelopes(HashMap::from_iter([(env1.id.clone(), env1.clone())]));
        let next_imap_envelopes = Envelopes(HashMap::from_iter([
            (env1.id.clone(), env1.clone()),
            (env2.id.clone(), env2.clone()),
        ]));
        let prev_mdir_envelopes = Envelopes(HashMap::from_iter([(env1.id.clone(), env1.clone())]));
        let next_mdir_envelopes = Envelopes(HashMap::from_iter([(env1.id.clone(), env1.clone())]));

        let patch = build_patch(
            prev_imap_envelopes,
            next_imap_envelopes,
            prev_mdir_envelopes,
            next_mdir_envelopes,
        );

        assert_eq!(vec![Hunk::Maildir(HunkKind::AddMsg("2".into()))], patch);
    }

    #[test]
    fn remove_mdir_msg_test() {
        let env1 = Envelope {
            id: "1".into(),
            flags: Flags(HashSet::from_iter([Flag::Seen])),
        };
        let env2 = Envelope {
            id: "2".into(),
            flags: Flags(HashSet::from_iter([Flag::Flagged])),
        };

        let prev_imap_envelopes = Envelopes(HashMap::from_iter([
            (env1.id.clone(), env1.clone()),
            (env2.id.clone(), env2.clone()),
        ]));
        let next_imap_envelopes = Envelopes(HashMap::from_iter([(env1.id.clone(), env1.clone())]));
        let prev_mdir_envelopes = Envelopes(HashMap::from_iter([
            (env1.id.clone(), env1.clone()),
            (env2.id.clone(), env2.clone()),
        ]));
        let next_mdir_envelopes = Envelopes(HashMap::from_iter([
            (env1.id.clone(), env1.clone()),
            (env2.id.clone(), env2.clone()),
        ]));

        let patch = build_patch(
            prev_imap_envelopes,
            next_imap_envelopes,
            prev_mdir_envelopes,
            next_mdir_envelopes,
        );

        assert_eq!(vec![Hunk::Maildir(HunkKind::RemoveMsg("2".into()))], patch);
    }

    #[test]
    fn single_add_remove_flag_tests() {
        let e1 = Envelope {
            id: "1".into(),
            flags: Flags(HashSet::from_iter([Flag::Seen, Flag::Replied])),
        };
        let e2 = Envelope {
            id: "1".into(),
            flags: Flags(HashSet::from_iter([
                Flag::Seen,
                Flag::Flagged,
                Flag::Replied,
            ])),
        };

        let imap_prev = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let imap_next = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let mdir_prev = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let mdir_next = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        assert_eq!(
            vec![Hunk::Imap(HunkKind::AddFlag("1".into(), Flag::Flagged))],
            build_patch(imap_prev, imap_next, mdir_prev, mdir_next),
        );

        let imap_prev = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let imap_next = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let mdir_prev = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let mdir_next = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        assert_eq!(
            vec![Hunk::Maildir(HunkKind::AddFlag("1".into(), Flag::Flagged))],
            build_patch(imap_prev, imap_next, mdir_prev, mdir_next),
        );

        let imap_prev = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let imap_next = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let mdir_prev = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let mdir_next = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        assert_eq!(
            vec![Hunk::Maildir(HunkKind::RemoveFlag(
                "1".into(),
                Flag::Flagged
            ))],
            build_patch(imap_prev, imap_next, mdir_prev, mdir_next),
        );

        let imap_prev = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let imap_next = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let mdir_prev = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let mdir_next = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        assert_eq!(
            vec![Hunk::Imap(HunkKind::RemoveFlag("1".into(), Flag::Flagged))],
            build_patch(imap_prev, imap_next, mdir_prev, mdir_next),
        );

        let imap_prev = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let imap_next = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let mdir_prev = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let mdir_next = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        assert_eq!(
            vec![Hunk::Maildir(HunkKind::AddFlag("1".into(), Flag::Flagged))],
            build_patch(imap_prev, imap_next, mdir_prev, mdir_next),
        );

        let imap_prev = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        let imap_next = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let mdir_prev = Envelopes(HashMap::from_iter([(e1.id.clone(), e1.clone())]));
        let mdir_next = Envelopes(HashMap::from_iter([(e2.id.clone(), e2.clone())]));
        assert_eq!(
            vec![Hunk::Maildir(HunkKind::RemoveFlag(
                "1".into(),
                Flag::Flagged
            ))],
            build_patch(imap_prev, imap_next, mdir_prev, mdir_next),
        );
    }
}
