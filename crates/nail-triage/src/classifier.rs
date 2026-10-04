//! The per-account Bayesian junk classifier.
//!
//! * **Scoring** (Gary Robinson's method, as in SpamBayes): every token
//!   the account has seen gets a probability `f(w)` from its junk / not
//!   junk counts, pulled towards 0.5 for rarely seen tokens
//!   ([`ClassifierParams::strength`]); the tokens that deviate most from
//!   0.5 are combined with Fisher's chi-square method into one
//!   probability. See [`score`].
//! * **Verdicts**: at or above [`JunkSettings::threshold`] a message is
//!   junk, at or below [`ClassifierParams::ham_cutoff`] it is not junk, in
//!   between it is **unsure** (left where it is, never moved). Nothing is
//!   classified before [`JunkSettings::min_training`] messages of each
//!   class were trained.
//! * **Training** by "Mark as junk / not junk" (and by the server's
//!   verdicts when honoured) goes through the [`JunkStore`] trait, which
//!   the core implements on its store; [`train`] returns a
//!   [`TrainingReceipt`] that [`undo`] reverts.
//! * **Never deletes**: the plans ([`plan_arrival`], [`plan_mark`]) only
//!   set keywords and move between the Inbox and the Junk folder.

use std::collections::{HashMap, HashSet};

use nail_api::triage::{JUNK_KEYWORD, JunkSettings, JunkSource, JunkVerdict, NOT_JUNK_KEYWORD};
use nail_api::{AccountId, FolderRole, MessageId};

use crate::ops::{FolderTarget, TriageOp};

/// Token counts `(junk, not junk)`: the number of trained messages of each
/// class a token occurred in.
pub type TokenCounts = HashMap<String, (u32, u32)>;

/// Messages trained per class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Trained {
    /// Trained as junk.
    pub junk: u32,
    /// Trained as not junk.
    pub ham: u32,
}

/// What a training step changed (mirrors the store's outcome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainOutcome {
    /// First training of this message.
    Trained,
    /// The message was trained as the other class before; it moved.
    Retrained,
    /// Already trained as this class; nothing changed.
    Unchanged,
}

/// Where the classifier's counts live. The core implements it on its
/// store (`Store::junk_token_counts`, `train_junk`, `junk_stats`; the
/// store hashes tokens); [`MemoryJunkStore`] is the in-memory reference.
pub trait JunkStore {
    /// The store's error.
    type Error;

    /// Counts of the given tokens in an account; unknown tokens are absent.
    fn token_counts(
        &self,
        account: &AccountId,
        tokens: &[String],
    ) -> Result<TokenCounts, Self::Error>;

    /// Messages trained per class.
    fn trained(&self, account: &AccountId) -> Result<Trained, Self::Error>;

    /// Trains one message (by key, see [`message_key`]) as junk or not;
    /// each distinct token counts once; a message trained as the other
    /// class before moves.
    fn train(
        &self,
        account: &AccountId,
        message_key: &str,
        junk: bool,
        tokens: &[String],
        now: i64,
    ) -> Result<TrainOutcome, Self::Error>;

    /// Forgets the training of one message. Returns whether it was
    /// trained.
    fn untrain(&self, account: &AccountId, message_key: &str) -> Result<bool, Self::Error>;
}

/// The key a message is trained under: its `Message-ID` plus a digest of
/// its tokens (so a copy in another folder is the same message, while
/// another message that reuses the `Message-ID` — the sender chooses it —
/// is trained on its own and never untrains the first), else
/// `local:<id>`.
pub fn message_key(rfc_message_id: Option<&str>, id: &MessageId, tokens: &[String]) -> String {
    use sha2::{Digest, Sha256};
    match rfc_message_id.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => {
            let mut unique: Vec<&str> = tokens.iter().map(String::as_str).collect();
            unique.sort_unstable();
            unique.dedup();
            let mut hash = Sha256::new();
            for token in unique {
                hash.update(token.as_bytes());
                hash.update([0]);
            }
            let digest = hex::encode(&hash.finalize()[..8]);
            format!("{}#{digest}", m.trim_matches(|c| c == '<' || c == '>'))
        }
        None => format!("local:{id}"),
    }
}

/// Tuning of the classifier (SpamBayes defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassifierParams {
    /// Robinson's `s`: how strongly the assumed probability weighs against
    /// few observations.
    pub strength: f64,
    /// Robinson's `x`: the probability assumed for an unknown token.
    pub assumed: f64,
    /// Tokens with `|f(w) - 0.5|` below this are ignored.
    pub min_deviation: f64,
    /// At most this many tokens (the most telling ones) are combined.
    pub max_clues: usize,
    /// Scores (percent) at or below this are "not junk"; between this and
    /// the account's threshold a message is "unsure".
    pub ham_cutoff: u8,
}

impl Default for ClassifierParams {
    fn default() -> Self {
        Self {
            strength: 0.45,
            assumed: 0.5,
            min_deviation: 0.1,
            max_clues: 150,
            ham_cutoff: 20,
        }
    }
}

/// A combined probability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// Junk probability 0.0–1.0.
    pub probability: f64,
    /// The same in percent, rounded (0–100).
    pub percent: u8,
    /// Tokens that contributed.
    pub clues: usize,
}

/// The class of a scored message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunkClass {
    /// At or above the threshold.
    Junk,
    /// Between the not-junk cutoff and the threshold.
    Unsure,
    /// At or below the not-junk cutoff.
    Ham,
}

/// The classifier's answer for one message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Classification {
    /// Not enough training yet.
    NotReady {
        /// Trained so far.
        trained: Trained,
        /// Needed per class.
        needed: u32,
    },
    /// Scored.
    Scored {
        /// The score.
        score: Score,
        /// Its class.
        class: JunkClass,
    },
}

impl Classification {
    /// The verdict to store and show (`None` when not scored). Unsure
    /// messages count as not junk (they stay where they are) but keep
    /// their score.
    pub fn verdict(&self) -> Option<JunkVerdict> {
        match self {
            Self::NotReady { .. } => None,
            Self::Scored { score, class } => Some(JunkVerdict {
                junk: *class == JunkClass::Junk,
                score: Some(score.percent),
                source: JunkSource::Classifier,
            }),
        }
    }
}

/// Robinson's `f(w)` of one token.
fn token_probability(p: &ClassifierParams, counts: (u32, u32), trained: Trained) -> Option<f64> {
    let (junk, ham) = counts;
    let n = f64::from(junk) + f64::from(ham);
    if n == 0.0 {
        return None;
    }
    // Ratios of the messages of each class that contain the token (capped:
    // counts can exceed the totals after an untrain race).
    let junk_ratio = if trained.junk == 0 {
        0.0
    } else {
        (f64::from(junk) / f64::from(trained.junk)).min(1.0)
    };
    let ham_ratio = if trained.ham == 0 {
        0.0
    } else {
        (f64::from(ham) / f64::from(trained.ham)).min(1.0)
    };
    let prob = if junk_ratio + ham_ratio == 0.0 {
        p.assumed
    } else {
        junk_ratio / (junk_ratio + ham_ratio)
    };
    Some((p.strength * p.assumed + n * prob) / (p.strength + n))
}

/// `P(chi² ≥ x2)` for `v` (even) degrees of freedom.
fn chi2_q(x2: f64, v: usize) -> f64 {
    let m = x2 / 2.0;
    let mut term = (-m).exp();
    let mut sum = term;
    let mut k = 1.0;
    for _ in 1..v / 2 {
        term *= m / k;
        sum += term;
        k += 1.0;
    }
    sum.min(1.0)
}

/// Combines the token probabilities of a message (Fisher / Robinson
/// chi-square). Only the distinct tokens count.
pub fn score(
    p: &ClassifierParams,
    tokens: &[String],
    counts: &TokenCounts,
    trained: Trained,
) -> Score {
    let mut seen = HashSet::new();
    let mut clues: Vec<f64> = tokens
        .iter()
        .filter(|t| seen.insert(t.as_str()))
        .filter_map(|t| counts.get(t))
        .filter_map(|c| token_probability(p, *c, trained))
        .filter(|f| (f - 0.5).abs() >= p.min_deviation)
        .collect();
    clues.sort_by(|a, b| (b - 0.5).abs().total_cmp(&(a - 0.5).abs()));
    clues.truncate(p.max_clues);
    let n = clues.len();
    let probability = if n == 0 {
        0.5
    } else {
        // Sums of logs (products underflow for long messages).
        let (mut ln_ham, mut ln_junk) = (0.0f64, 0.0f64);
        for f in &clues {
            let f = f.clamp(1e-6, 1.0 - 1e-6);
            ln_ham += f.ln();
            ln_junk += (1.0 - f).ln();
        }
        let s = 1.0 - chi2_q(-2.0 * ln_junk, 2 * n);
        let h = 1.0 - chi2_q(-2.0 * ln_ham, 2 * n);
        f64::midpoint(s - h, 1.0).clamp(0.0, 1.0)
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "probability is clamped to 0..=1, so the rounded percent is 0..=100"
    )]
    let percent = (probability * 100.0).round() as u8;
    Score {
        probability,
        percent,
        clues: n,
    }
}

/// The class of a score under an account's threshold.
pub fn class_of(p: &ClassifierParams, settings: &JunkSettings, percent: u8) -> JunkClass {
    let threshold = settings.threshold.clamp(1, 100);
    if percent >= threshold {
        JunkClass::Junk
    } else if percent <= p.ham_cutoff.min(threshold - 1) {
        JunkClass::Ham
    } else {
        JunkClass::Unsure
    }
}

/// Classifies a message of an account from its tokens.
pub fn classify<S: JunkStore>(
    store: &S,
    params: &ClassifierParams,
    account: &AccountId,
    settings: &JunkSettings,
    tokens: &[String],
) -> Result<Classification, S::Error> {
    let trained = store.trained(account)?;
    let needed = settings.min_training.max(1);
    if trained.junk < needed || trained.ham < needed {
        return Ok(Classification::NotReady { trained, needed });
    }
    let counts = store.token_counts(account, tokens)?;
    let scored = score(params, tokens, &counts, trained);
    Ok(Classification::Scored {
        score: scored,
        class: class_of(params, settings, scored.percent),
    })
}

/// What [`train`] did, so it can be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainingReceipt {
    /// The account.
    pub account_id: AccountId,
    /// The message key.
    pub message_key: String,
    /// The class it was trained as.
    pub junk: bool,
    /// Its class before (`None` = untrained).
    pub previous: Option<bool>,
}

/// Trains a message as junk or not junk.
pub fn train<S: JunkStore>(
    store: &S,
    account: &AccountId,
    message_key: &str,
    junk: bool,
    tokens: &[String],
    now: i64,
) -> Result<TrainingReceipt, S::Error> {
    let outcome = store.train(account, message_key, junk, tokens, now)?;
    Ok(TrainingReceipt {
        account_id: account.clone(),
        message_key: message_key.to_owned(),
        junk,
        previous: match outcome {
            TrainOutcome::Trained => None,
            TrainOutcome::Retrained => Some(!junk),
            TrainOutcome::Unchanged => Some(junk),
        },
    })
}

/// Reverts a training step (the undo of "Mark as junk / not junk").
/// `tokens` are the message's tokens (needed when it goes back to its
/// earlier class).
pub fn undo<S: JunkStore>(
    store: &S,
    receipt: &TrainingReceipt,
    tokens: &[String],
    now: i64,
) -> Result<(), S::Error> {
    match receipt.previous {
        None => {
            store.untrain(&receipt.account_id, &receipt.message_key)?;
        }
        Some(previous) if previous != receipt.junk => {
            store.train(
                &receipt.account_id,
                &receipt.message_key,
                previous,
                tokens,
                now,
            )?;
        }
        Some(_) => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plans

/// Where a message is and what the server says about it.
#[derive(Debug, Clone, Default)]
pub struct MessageFacts<'a> {
    /// The message.
    pub message_id: Option<&'a MessageId>,
    /// Role of its folder.
    pub folder_role: FolderRole,
    /// Its IMAP keywords (any case).
    pub keywords: &'a [String],
}

impl MessageFacts<'_> {
    fn has(&self, keyword: &str) -> bool {
        self.keywords
            .iter()
            .any(|k| k.eq_ignore_ascii_case(keyword))
    }

    /// The server's verdict: `$Junk` / delivery into Junk → junk,
    /// `$NotJunk` → not junk (it wins over the folder: the user rescued it
    /// elsewhere), nothing → `None`.
    pub fn server_verdict(&self) -> Option<bool> {
        if self.has(NOT_JUNK_KEYWORD) {
            Some(false)
        } else if self.has(JUNK_KEYWORD) || self.folder_role == FolderRole::Junk {
            Some(true)
        } else {
            None
        }
    }
}

/// What to do with an arriving message.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JunkDecision {
    /// The verdict to store (`None` = no verdict).
    pub verdict: Option<JunkVerdict>,
    /// Train the classifier as this class (server verdicts, when
    /// honoured).
    pub train_as: Option<bool>,
    /// The writes.
    pub ops: Vec<TriageOp>,
}

/// Decides about an arriving message. The server's verdict wins when
/// [`JunkSettings::honor_server_flags`] is on; otherwise the classifier's
/// (`classification`, only when [`JunkSettings::enabled`]). Junk gets the
/// `$Junk` keyword and — with [`JunkSettings::move_to_junk`] — moves to
/// the Junk folder; nothing is ever deleted.
pub fn plan_arrival(
    settings: &JunkSettings,
    facts: &MessageFacts<'_>,
    account: &AccountId,
    classification: Option<&Classification>,
) -> JunkDecision {
    let ids: Vec<MessageId> = facts.message_id.cloned().into_iter().collect();
    if settings.honor_server_flags
        && let Some(junk) = facts.server_verdict()
    {
        let mut ops = Vec::new();
        if junk && settings.move_to_junk && facts.folder_role == FolderRole::Inbox {
            ops.push(move_to(&ids, account, FolderRole::Junk));
        }
        return JunkDecision {
            verdict: Some(JunkVerdict {
                junk,
                score: None,
                source: JunkSource::Server,
            }),
            train_as: Some(junk),
            ops,
        };
    }
    if !settings.enabled {
        return JunkDecision::default();
    }
    let Some(verdict) = classification.and_then(Classification::verdict) else {
        return JunkDecision::default();
    };
    let mut ops = Vec::new();
    if verdict.junk {
        ops.push(TriageOp::AddKeyword {
            message_ids: ids.clone(),
            keyword: JUNK_KEYWORD.to_owned(),
        });
        if settings.move_to_junk && facts.folder_role != FolderRole::Junk {
            ops.push(move_to(&ids, account, FolderRole::Junk));
        }
    }
    JunkDecision {
        verdict: Some(verdict),
        train_as: None,
        ops,
    }
}

/// The writes of "Mark as junk" / "Mark as not junk" for one message:
/// keywords, and a move to Junk (with [`JunkSettings::move_to_junk`]) or
/// back to the Inbox from Junk. Training is separate ([`train`]).
pub fn plan_mark(
    settings: &JunkSettings,
    facts: &MessageFacts<'_>,
    account: &AccountId,
    junk: bool,
) -> Vec<TriageOp> {
    let ids: Vec<MessageId> = facts.message_id.cloned().into_iter().collect();
    let (add, remove) = if junk {
        (JUNK_KEYWORD, NOT_JUNK_KEYWORD)
    } else {
        (NOT_JUNK_KEYWORD, JUNK_KEYWORD)
    };
    let mut ops = Vec::new();
    if facts.has(remove) {
        ops.push(TriageOp::RemoveKeyword {
            message_ids: ids.clone(),
            keyword: remove.to_owned(),
        });
    }
    if !facts.has(add) {
        ops.push(TriageOp::AddKeyword {
            message_ids: ids.clone(),
            keyword: add.to_owned(),
        });
    }
    if junk && settings.move_to_junk && facts.folder_role != FolderRole::Junk {
        ops.push(move_to(&ids, account, FolderRole::Junk));
    }
    if !junk && facts.folder_role == FolderRole::Junk {
        ops.push(move_to(&ids, account, FolderRole::Inbox));
    }
    ops
}

fn move_to(ids: &[MessageId], account: &AccountId, role: FolderRole) -> TriageOp {
    TriageOp::Move {
        message_ids: ids.to_vec(),
        to: FolderTarget::Role {
            account_id: account.clone(),
            role,
        },
    }
}

// ---------------------------------------------------------------------------
// In-memory store

/// An in-memory [`JunkStore`] with the store's semantics (distinct tokens
/// once per message, retraining moves a message between classes). For
/// tests and as the reference the core's adapter is checked against.
#[derive(Debug, Default)]
pub struct MemoryJunkStore {
    inner: std::sync::Mutex<MemoryInner>,
}

#[derive(Debug, Default)]
struct MemoryInner {
    tokens: HashMap<(AccountId, String), (u32, u32)>,
    training: HashMap<(AccountId, String), (bool, Vec<String>)>,
}

impl MemoryJunkStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Distinct tokens of an account.
    pub fn token_total(&self, account: &AccountId) -> usize {
        self.lock()
            .tokens
            .keys()
            .filter(|(a, _)| a == account)
            .count()
    }
}

impl MemoryInner {
    fn remove(&mut self, account: &AccountId, key: &str) -> Option<bool> {
        let (junk, tokens) = self.training.remove(&(account.clone(), key.to_owned()))?;
        for token in tokens {
            let k = (account.clone(), token);
            if let Some(c) = self.tokens.get_mut(&k) {
                if junk {
                    c.0 = c.0.saturating_sub(1);
                } else {
                    c.1 = c.1.saturating_sub(1);
                }
                if *c == (0, 0) {
                    self.tokens.remove(&k);
                }
            }
        }
        Some(junk)
    }
}

impl JunkStore for MemoryJunkStore {
    type Error = std::convert::Infallible;

    fn token_counts(
        &self,
        account: &AccountId,
        tokens: &[String],
    ) -> Result<TokenCounts, Self::Error> {
        let inner = self.lock();
        Ok(tokens
            .iter()
            .filter_map(|t| {
                inner
                    .tokens
                    .get(&(account.clone(), t.clone()))
                    .map(|c| (t.clone(), *c))
            })
            .collect())
    }

    fn trained(&self, account: &AccountId) -> Result<Trained, Self::Error> {
        let inner = self.lock();
        let mut out = Trained::default();
        for ((a, _), (junk, _)) in &inner.training {
            if a == account {
                if *junk {
                    out.junk += 1;
                } else {
                    out.ham += 1;
                }
            }
        }
        Ok(out)
    }

    fn train(
        &self,
        account: &AccountId,
        message_key: &str,
        junk: bool,
        tokens: &[String],
        _now: i64,
    ) -> Result<TrainOutcome, Self::Error> {
        let mut inner = self.lock();
        let outcome = match inner
            .training
            .get(&(account.clone(), message_key.to_owned()))
        {
            Some((was, _)) if *was == junk => return Ok(TrainOutcome::Unchanged),
            Some(_) => TrainOutcome::Retrained,
            None => TrainOutcome::Trained,
        };
        inner.remove(account, message_key);
        let mut distinct: Vec<String> = tokens.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        for token in &distinct {
            let c = inner
                .tokens
                .entry((account.clone(), token.clone()))
                .or_default();
            if junk {
                c.0 += 1;
            } else {
                c.1 += 1;
            }
        }
        inner
            .training
            .insert((account.clone(), message_key.to_owned()), (junk, distinct));
        Ok(outcome)
    }

    fn untrain(&self, account: &AccountId, message_key: &str) -> Result<bool, Self::Error> {
        Ok(self.lock().remove(account, message_key).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenize::{TokenSource, Tokenizer};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn toks(text: &str) -> Vec<String> {
        Tokenizer::default().tokens(TokenSource { headers: &[], text })
    }

    fn acct() -> AccountId {
        "work".into()
    }

    const SPAM: [&str; 6] = [
        "cheap pills viagra discount pharmacy order now",
        "win money lottery winner claim prize now",
        "cheap watches replica discount luxury order",
        "casino bonus free spins winner money",
        "discount pharmacy pills cheap no prescription",
        "claim your free prize money winner lottery",
    ];
    const HAM: [&str; 6] = [
        "meeting tomorrow about the project schedule",
        "please review the attached quarterly report draft",
        "lunch on friday with the team",
        "the project deadline moved to next week",
        "notes from the meeting and action items",
        "can you review my draft before friday",
    ];

    fn trained_store(times: usize) -> MemoryJunkStore {
        let store = MemoryJunkStore::new();
        for round in 0..times {
            for (i, text) in SPAM.iter().enumerate() {
                train(
                    &store,
                    &acct(),
                    &format!("s{round}-{i}"),
                    true,
                    &toks(text),
                    0,
                )
                .unwrap();
            }
            for (i, text) in HAM.iter().enumerate() {
                train(
                    &store,
                    &acct(),
                    &format!("h{round}-{i}"),
                    false,
                    &toks(text),
                    0,
                )
                .unwrap();
            }
        }
        store
    }

    fn settings() -> JunkSettings {
        JunkSettings {
            enabled: true,
            min_training: 5,
            ..JunkSettings::default()
        }
    }

    #[test]
    fn chi2_matches_known_values() {
        // Q(x2 = 2, v = 2) = e^-1.
        assert!((chi2_q(2.0, 2) - (-1.0f64).exp()).abs() < 1e-12);
        assert!((chi2_q(0.0, 10) - 1.0).abs() < 1e-12);
        assert!(chi2_q(1e6, 4) < 1e-12);
        // Q(4, 4) = e^-2 (1 + 2).
        assert!((chi2_q(4.0, 4) - 3.0 * (-2.0f64).exp()).abs() < 1e-12);
    }

    #[test]
    fn robinson_probability() {
        let p = ClassifierParams::default();
        let t = Trained { junk: 10, ham: 10 };
        assert_eq!(token_probability(&p, (0, 0), t), None);
        let f = token_probability(&p, (5, 0), t).unwrap();
        // (0.45 * 0.5 + 5 * 1) / 5.45
        assert!((f - (0.225 + 5.0) / 5.45).abs() < 1e-12);
        let even = token_probability(&p, (3, 3), t).unwrap();
        assert!((even - 0.5).abs() < 1e-12);
        // Unbalanced training: ratios, not raw counts.
        let skew = token_probability(&p, (1, 10), Trained { junk: 1, ham: 100 }).unwrap();
        assert!(skew > 0.8, "{skew}");
    }

    #[test]
    fn not_ready_before_min_training() {
        let store = trained_store(0);
        let c = classify(
            &store,
            &ClassifierParams::default(),
            &acct(),
            &settings(),
            &toks("x"),
        )
        .unwrap();
        assert_eq!(
            c,
            Classification::NotReady {
                trained: Trained::default(),
                needed: 5
            }
        );
        assert_eq!(c.verdict(), None);
    }

    #[test]
    fn separates_spam_and_ham() {
        let store = trained_store(1);
        let p = ClassifierParams::default();
        let s = settings();
        let spam = classify(
            &store,
            &p,
            &acct(),
            &s,
            &toks("cheap discount pills winner money prize"),
        )
        .unwrap();
        let ham = classify(
            &store,
            &p,
            &acct(),
            &s,
            &toks("project meeting review draft friday"),
        )
        .unwrap();
        let unknown = classify(&store, &p, &acct(), &s, &toks("zebra xylophone")).unwrap();
        let Classification::Scored { score, class } = spam else {
            panic!("{spam:?}")
        };
        assert_eq!(class, JunkClass::Junk, "{score:?}");
        assert!(score.percent >= 90);
        let Classification::Scored { score, class } = ham else {
            panic!("{ham:?}")
        };
        assert_eq!(class, JunkClass::Ham, "{score:?}");
        assert!(score.percent <= 10);
        let Classification::Scored { score, class } = unknown else {
            panic!()
        };
        assert_eq!(
            (score.percent, score.clues, class),
            (50, 0, JunkClass::Unsure)
        );
        assert_eq!(
            unknown.verdict(),
            Some(JunkVerdict {
                junk: false,
                score: Some(50),
                source: JunkSource::Classifier
            })
        );
        // Other accounts know nothing.
        let other = classify(&store, &p, &"home".into(), &s, &toks("cheap pills")).unwrap();
        assert!(matches!(other, Classification::NotReady { .. }));
    }

    #[test]
    fn mixed_message_is_unsure() {
        let store = trained_store(2);
        let p = ClassifierParams::default();
        let c = classify(
            &store,
            &p,
            &acct(),
            &settings(),
            &toks("cheap pills discount pharmacy meeting project review draft report"),
        )
        .unwrap();
        let Classification::Scored { class, score } = c else {
            panic!()
        };
        assert_eq!(class, JunkClass::Unsure, "{score:?}");
    }

    #[test]
    fn classes_follow_threshold_and_cutoff() {
        let p = ClassifierParams::default();
        let s = JunkSettings {
            threshold: 80,
            ..JunkSettings::default()
        };
        assert_eq!(class_of(&p, &s, 80), JunkClass::Junk);
        assert_eq!(class_of(&p, &s, 79), JunkClass::Unsure);
        assert_eq!(class_of(&p, &s, 21), JunkClass::Unsure);
        assert_eq!(class_of(&p, &s, 20), JunkClass::Ham);
        // A cutoff above the threshold never overlaps it.
        let wide = ClassifierParams {
            ham_cutoff: 95,
            ..p
        };
        assert_eq!(class_of(&wide, &s, 79), JunkClass::Ham);
        assert_eq!(class_of(&wide, &s, 80), JunkClass::Junk);
    }

    #[test]
    fn training_retraining_and_undo() {
        let store = MemoryJunkStore::new();
        let a = acct();
        let t = toks("cheap pills cheap");
        let r1 = train(&store, &a, "<m1@x>", true, &t, 1).unwrap();
        assert_eq!(r1.previous, None);
        assert_eq!(store.trained(&a).unwrap(), Trained { junk: 1, ham: 0 });
        assert_eq!(
            store.token_counts(&a, &t).unwrap()["cheap"],
            (1, 0),
            "distinct tokens count once"
        );
        // Same class again: unchanged.
        let r2 = train(&store, &a, "<m1@x>", true, &t, 2).unwrap();
        assert_eq!(r2.previous, Some(true));
        // Other class: moves.
        let r3 = train(&store, &a, "<m1@x>", false, &t, 3).unwrap();
        assert_eq!(r3.previous, Some(true));
        assert_eq!(store.trained(&a).unwrap(), Trained { junk: 0, ham: 1 });
        assert_eq!(store.token_counts(&a, &t).unwrap()["cheap"], (0, 1));
        // Undo the move: back to junk.
        undo(&store, &r3, &t, 4).unwrap();
        assert_eq!(store.token_counts(&a, &t).unwrap()["cheap"], (1, 0));
        // Undo an unchanged step: nothing.
        undo(&store, &r2, &t, 5).unwrap();
        assert_eq!(store.trained(&a).unwrap(), Trained { junk: 1, ham: 0 });
        // Undo the first training: forgotten.
        undo(&store, &r1, &t, 6).unwrap();
        assert_eq!(store.trained(&a).unwrap(), Trained::default());
        assert!(store.token_counts(&a, &t).unwrap().is_empty());
        assert_eq!(store.token_total(&a), 0);
        assert!(!store.untrain(&a, "<m1@x>").unwrap());
    }

    #[test]
    fn message_keys() {
        let t = kw(&["cheap", "pills", "cheap"]);
        let key = message_key(Some(" <a@b> "), &"m1".into(), &t);
        assert!(
            key.starts_with("a@b#") && key.len() == "a@b#".len() + 16,
            "{key}"
        );
        // A copy (same content, other id, token order) has the same key.
        assert_eq!(
            message_key(Some("<a@b>"), &"m2".into(), &kw(&["pills", "cheap"])),
            key
        );
        // Another message reusing the Message-ID gets its own key.
        assert_ne!(
            message_key(Some("<a@b>"), &"m3".into(), &kw(&["meeting", "notes"])),
            key
        );
        assert_eq!(message_key(Some(""), &"m1".into(), &t), "local:m1");
        assert_eq!(message_key(None, &"m1".into(), &t), "local:m1");
    }

    fn kw(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn all_ops_safe(ops: &[TriageOp]) {
        assert!(ops.iter().all(|o| !o.targets_trash()), "{ops:?}");
    }

    #[test]
    fn arrival_honours_server_flags_first() {
        let id = MessageId::from("m1");
        let a = acct();
        let flagged = kw(&["$junk"]);
        let facts = MessageFacts {
            message_id: Some(&id),
            folder_role: FolderRole::Inbox,
            keywords: &flagged,
        };
        let ham_class = Classification::Scored {
            score: Score {
                probability: 0.01,
                percent: 1,
                clues: 3,
            },
            class: JunkClass::Ham,
        };
        let d = plan_arrival(&settings(), &facts, &a, Some(&ham_class));
        assert_eq!(d.verdict.as_ref().unwrap().source, JunkSource::Server);
        assert!(d.verdict.as_ref().unwrap().junk);
        assert_eq!(d.train_as, Some(true));
        assert_eq!(
            d.ops,
            [TriageOp::Move {
                message_ids: vec![id.clone()],
                to: FolderTarget::Role {
                    account_id: a.clone(),
                    role: FolderRole::Junk
                }
            }]
        );
        // Delivered into Junk by the server: verdict, no move.
        let in_junk = MessageFacts {
            message_id: Some(&id),
            folder_role: FolderRole::Junk,
            keywords: &[],
        };
        let d = plan_arrival(&settings(), &in_junk, &a, None);
        assert_eq!(d.train_as, Some(true));
        assert!(d.ops.is_empty());
        // $NotJunk wins over the folder.
        let rescued = kw(&["$NotJunk"]);
        let facts = MessageFacts {
            keywords: &rescued,
            ..in_junk.clone()
        };
        assert_eq!(facts.server_verdict(), Some(false));
        // Not honoured: the classifier decides.
        let s = JunkSettings {
            honor_server_flags: false,
            ..settings()
        };
        let d = plan_arrival(&s, &in_junk, &a, Some(&ham_class));
        assert_eq!(d.verdict.unwrap().source, JunkSource::Classifier);
        assert!(d.ops.is_empty());
    }

    #[test]
    fn arrival_classifier_moves_junk_only_when_enabled() {
        let id = MessageId::from("m1");
        let a = acct();
        let facts = MessageFacts {
            message_id: Some(&id),
            folder_role: FolderRole::Inbox,
            keywords: &[],
        };
        let junk = Classification::Scored {
            score: Score {
                probability: 0.99,
                percent: 99,
                clues: 9,
            },
            class: JunkClass::Junk,
        };
        let d = plan_arrival(&settings(), &facts, &a, Some(&junk));
        assert_eq!(d.verdict.as_ref().unwrap().score, Some(99));
        assert_eq!(d.train_as, None, "the classifier never trains itself");
        assert_eq!(d.ops.len(), 2);
        all_ops_safe(&d.ops);
        let keep = JunkSettings {
            move_to_junk: false,
            ..settings()
        };
        assert_eq!(plan_arrival(&keep, &facts, &a, Some(&junk)).ops.len(), 1);
        let off = JunkSettings {
            enabled: false,
            ..settings()
        };
        assert_eq!(
            plan_arrival(&off, &facts, &a, Some(&junk)),
            JunkDecision::default()
        );
        let not_ready = Classification::NotReady {
            trained: Trained::default(),
            needed: 10,
        };
        assert_eq!(
            plan_arrival(&settings(), &facts, &a, Some(&not_ready)),
            JunkDecision::default()
        );
    }

    #[test]
    fn marking_plans_keywords_and_moves() {
        let id = MessageId::from("m1");
        let a = acct();
        let was = kw(&["$NotJunk"]);
        let facts = MessageFacts {
            message_id: Some(&id),
            folder_role: FolderRole::Inbox,
            keywords: &was,
        };
        let ops = plan_mark(&settings(), &facts, &a, true);
        assert_eq!(
            ops,
            [
                TriageOp::RemoveKeyword {
                    message_ids: vec![id.clone()],
                    keyword: "$NotJunk".into()
                },
                TriageOp::AddKeyword {
                    message_ids: vec![id.clone()],
                    keyword: "$Junk".into()
                },
                TriageOp::Move {
                    message_ids: vec![id.clone()],
                    to: FolderTarget::Role {
                        account_id: a.clone(),
                        role: FolderRole::Junk
                    }
                },
            ]
        );
        let junk_kw = kw(&["$Junk"]);
        let in_junk = MessageFacts {
            message_id: Some(&id),
            folder_role: FolderRole::Junk,
            keywords: &junk_kw,
        };
        let ops = plan_mark(&settings(), &in_junk, &a, false);
        assert_eq!(ops.len(), 3);
        assert!(matches!(
            &ops[2],
            TriageOp::Move {
                to: FolderTarget::Role {
                    role: FolderRole::Inbox,
                    ..
                },
                ..
            }
        ));
        // Not junk outside Junk: keywords only.
        let ops = plan_mark(&settings(), &facts, &a, false);
        assert!(ops.is_empty(), "{ops:?}");
    }

    proptest! {
        /// Whatever the facts and settings, no plan ever deletes or trashes.
        #[test]
        fn plans_never_delete(
            role in prop::sample::select(vec![
                FolderRole::Inbox, FolderRole::Junk, FolderRole::Trash,
                FolderRole::Archive, FolderRole::Other,
            ]),
            keywords in prop::collection::vec(
                prop::sample::select(vec!["$Junk", "$NotJunk", "\\Seen", "$junk"]), 0..3),
            enabled: bool, honor: bool, move_to_junk: bool, junk: bool, percent in 0u8..=100,
        ) {
            let id = MessageId::from("m");
            let keywords: Vec<String> = keywords.iter().map(|k| (*k).to_owned()).collect();
            let facts = MessageFacts { message_id: Some(&id), folder_role: role, keywords: &keywords };
            let s = JunkSettings { enabled, honor_server_flags: honor, move_to_junk, ..JunkSettings::default() };
            let p = ClassifierParams::default();
            let c = Classification::Scored {
                score: Score { probability: f64::from(percent) / 100.0, percent, clues: 1 },
                class: class_of(&p, &s, percent),
            };
            for ops in [plan_arrival(&s, &facts, &acct(), Some(&c)).ops, plan_mark(&s, &facts, &acct(), junk)] {
                prop_assert!(ops.iter().all(|o| !o.targets_trash()));
                for op in &ops {
                    prop_assert_eq!(op.message_ids(), std::slice::from_ref(&id));
                }
            }
        }

        /// Scores stay in range and adding junk evidence never lowers it.
        #[test]
        fn score_is_bounded_and_monotone(
            counts in prop::collection::vec((0u32..50, 0u32..50), 0..40),
            extra in 1u32..50,
        ) {
            let p = ClassifierParams::default();
            let trained = Trained { junk: 50, ham: 50 };
            let tokens: Vec<String> = (0..counts.len()).map(|i| format!("t{i}")).collect();
            let map: TokenCounts = tokens.iter().cloned().zip(counts.iter().copied()).collect();
            let s = score(&p, &tokens, &map, trained);
            prop_assert!((0.0..=1.0).contains(&s.probability));
            prop_assert!(s.percent <= 100);
            prop_assert!(s.clues <= counts.len());
            let mut more = tokens.clone();
            more.push("spammy".to_owned());
            let mut map2 = map.clone();
            map2.insert("spammy".to_owned(), (extra.min(50), 0));
            let s2 = score(&p, &more, &map2, trained);
            prop_assert!(s2.probability + 1e-9 >= s.probability || s.clues >= p.max_clues,
                "{} < {}", s2.probability, s.probability);
        }
    }
}
