use itertools::Itertools;
use serenity::model::channel::Message;
use serenity::model::id::{ChannelId, UserId};
use serenity::prelude::*;
use serenity::utils::Colour;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Display;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_CONTEST_DURATION: Duration = Duration::from_secs(60);

const ALLOWED_DURATION_RANGE: RangeInclusive<Duration> =
    Duration::from_secs(10)..=Duration::from_secs(60 * 60);

const PIN_ANNOUNCEMENT_THRESHOLD: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct Handler {
    contests: Arc<Mutex<HashMap<ChannelId, Contest>>>,
}

impl Handler {
    pub fn new() -> Self {
        Self::default()
    }
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if let Some(contest) = self.contests.lock().await.get_mut(&msg.channel_id) {
            contest.count(&msg);
            return;
        }

        if msg.content.to_lowercase().contains("spam") {
            let duration = msg
                .content
                .split_ascii_whitespace()
                .filter_map(|w| w.parse().ok())
                .map(Duration::from_secs)
                .filter(|d| ALLOWED_DURATION_RANGE.contains(d))
                .next()
                .unwrap_or(DEFAULT_CONTEST_DURATION);

            if let Err(err) = run_contest(
                ctx,
                msg.channel_id,
                duration,
                &self.contests,
                duration >= PIN_ANNOUNCEMENT_THRESHOLD,
            )
            .await
            {
                eprintln!("Error: {:?}", err)
            }
        }
    }
}

#[derive(Default, PartialEq, Eq)]
pub struct SpamCount {
    messages: usize,
    characters: usize,
}

#[derive(Default)]
pub struct Contest {
    counts: HashMap<UserId, SpamCount>,
}

impl Contest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&mut self, message: &Message) {
        match self.counts.get_mut(&message.author.id) {
            None => {
                self.counts.insert(
                    message.author.id,
                    SpamCount {
                        messages: 1,
                        characters: message.content.len(),
                    },
                );
            }
            Some(count) => {
                count.messages += 1;
                count.characters += message.content.len()
            }
        };
    }

    pub fn ranking_by<F, K, D, S>(&self, kf: F, df: D) -> String
    where
        F: Fn(&SpamCount) -> K,
        K: Ord,
        D: Fn(&SpamCount) -> S,
        S: Display,
    {
        let mut ranking = self.counts.iter().collect::<Vec<_>>();
        ranking.sort_unstable_by_key(|(_, c)| kf(c));

        let mut result = String::new();
        let mut cur_rank_num = 1;
        for (_, rank_group) in &ranking.into_iter().group_by(|elt| kf((*elt).1)) {
            let mut group_size = 0;
            for (userid, count) in rank_group {
                result.push_str(
                    format!("**{}.:** <@{}> ({})\n", cur_rank_num, userid, df(count)).as_str(),
                );
                group_size += 1;
            }
            cur_rank_num += group_size;
        }
        result
    }
}

pub async fn run_contest(
    ctx: Context,
    channel_id: ChannelId,
    duration: Duration,
    contests: &Arc<Mutex<HashMap<ChannelId, Contest>>>,
    pin_announcement: bool,
) -> serenity::Result<()> {
    let end_timestamp =
        (chrono::Utc::now() + chrono::Duration::from_std(duration).unwrap()).timestamp() + 1;

    // send announcement message
    let announcement = channel_id
        .send_message(&ctx.http, |m| {
            m.embed(|e| {
                e.title("Es wurde ein Spam-Wettbewerb gestartet!")
                    .description(format!(
                        "Wer am meisten spamt, gewinnt.\nEnde <t:{}:R>.",
                        end_timestamp
                    ))
                    .colour(Colour::BLUE)
            })
        })
        .await?;

    if pin_announcement {
        announcement.pin(&ctx.http).await?;
    }

    contests.lock().await.insert(channel_id, Contest::new());
    tokio::time::sleep(duration).await;
    let contest = contests.lock().await.remove(&channel_id).unwrap();

    if contest.counts.is_empty() {
        announcement.delete(&ctx.http).await?;
    } else {
        if pin_announcement {
            announcement.unpin(&ctx.http).await?;
        }

        // send ranking message
        channel_id
            .send_message(&ctx.http, |m| {
                m.embed(|e| {
                    e.title("Der Wettbewerb ist beendet!")
                        .colour(Colour::DARK_GREEN)
                        .field(
                            "Ergebnisse (nach Nachrichten):",
                            contest.ranking_by(|c| Reverse(c.messages), |c| c.messages),
                            false,
                        )
                        .field(
                            "Ergebnisse (nach Zeichen):",
                            contest.ranking_by(|c| Reverse(c.characters), |c| c.characters),
                            false,
                        )
                })
            })
            .await?;
    }

    Ok(())
}
