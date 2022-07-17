use itertools::Itertools;
use log::{debug, error, info};
use serenity::client::{Context, EventHandler};
use serenity::model::channel::Message;
use serenity::model::event::ResumedEvent;
use serenity::model::gateway::{Activity, Ready};
use serenity::model::id::{ChannelId, UserId};
use serenity::utils::Colour;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Display;
use std::ops::RangeInclusive;
use std::sync::Mutex;
use std::time::Duration;

const DEFAULT_CONTEST_DURATION: Duration = Duration::from_secs(60);

const ALLOWED_DURATION_RANGE: RangeInclusive<Duration> =
    Duration::from_secs(10)..=Duration::from_secs(60 * 60);

const PIN_ANNOUNCEMENT_THRESHOLD: Duration = Duration::from_secs(5 * 60);

type Contests = Mutex<HashMap<ChannelId, Contest>>;

#[derive(Default)]
pub struct Handler {
    contests: Contests,
}

impl Handler {
    pub fn new() -> Self {
        Self::default()
    }
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if let Some(contest) = self.contests.lock().unwrap().get_mut(&msg.channel_id) {
            debug!(
                "Counting message {} (from {} in channel {})",
                msg.id,
                msg.author.tag(),
                msg.channel_id.0
            );
            contest.count(&msg);
            return;
        }

        if msg.content.to_lowercase().contains("spam") {
            let duration = msg
                .content
                .split_ascii_whitespace()
                .filter_map(|w| w.parse().ok())
                .map(Duration::from_secs)
                .find(|d| ALLOWED_DURATION_RANGE.contains(d))
                .unwrap_or(DEFAULT_CONTEST_DURATION);

            info!(
                "User {} started a {} second contest in channel {}",
                msg.author.tag(),
                duration.as_secs(),
                msg.channel_id.0
            );

            match run_contest(
                ctx,
                msg.channel_id,
                duration,
                &self.contests,
                duration >= PIN_ANNOUNCEMENT_THRESHOLD,
            )
            .await
            {
                Ok(contest) => debug!(
                    "Contest in channel {} has ended with {} participant(s)",
                    msg.channel_id.0,
                    contest.counts.len()
                ),
                Err(err) => error!(
                    "Error while running contest in channel {}: {}",
                    msg.channel_id.0, err
                ),
            };
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Connected as {}", ready.user.tag());
        ctx.set_activity(Activity::listening("Spam")).await;
    }

    async fn resume(&self, _ctx: Context, _: ResumedEvent) {
        info!("Resumed");
    }
}

#[derive(Default, PartialEq, Eq)]
struct SpamCount {
    messages: usize,
    characters: usize,
}

#[derive(Default)]
struct Contest {
    counts: HashMap<UserId, SpamCount>,
}

impl Contest {
    fn new() -> Self {
        Self::default()
    }

    fn count(&mut self, message: &Message) {
        let char_count = message.content.chars().count();
        match self.counts.get_mut(&message.author.id) {
            None => {
                self.counts.insert(
                    message.author.id,
                    SpamCount {
                        messages: 1,
                        characters: char_count,
                    },
                );
            }
            Some(count) => {
                count.messages += 1;
                count.characters += char_count;
            }
        };
    }

    fn ranking_by<Fk, K, Fd, D>(&self, fk: Fk, fd: Fd) -> String
    where
        Fk: Fn(&SpamCount) -> K,
        K: Ord,
        Fd: Fn(&SpamCount) -> D,
        D: Display,
    {
        let mut ranking = self.counts.iter().collect::<Vec<_>>();
        ranking.sort_unstable_by_key(|(_, c)| fk(c));

        let mut result = String::new();
        let mut cur_rank_num = 1;
        for (_, rank_group) in &ranking.into_iter().group_by(|elt| fk((*elt).1)) {
            let mut group_size = 0;
            for (userid, count) in rank_group {
                result.push_str(
                    format!("**{cur_rank_num}.:** <@{userid}> ({})\n", fd(count)).as_str(),
                );
                group_size += 1;
            }
            cur_rank_num += group_size;
        }
        result
    }
}

async fn run_contest(
    ctx: Context,
    channel_id: ChannelId,
    duration: Duration,
    contests: &Contests,
    pin: bool,
) -> serenity::Result<Contest> {
    let end_timestamp =
        (chrono::Utc::now() + chrono::Duration::from_std(duration).unwrap()).timestamp() + 1;

    // send announcement message
    let announcement = channel_id
        .send_message(&ctx.http, |m| {
            m.embed(|e| {
                e.title("Es wurde ein Spam-Wettbewerb gestartet!")
                    .description(format!(
                        "Wer am meisten spamt, gewinnt.\nEnde <t:{end_timestamp}:R>.",
                    ))
                    .colour(Colour::BLUE)
            })
        })
        .await?;

    if pin {
        announcement.pin(&ctx.http).await.ok();
    }

    contests.lock().unwrap().insert(channel_id, Contest::new());
    tokio::time::sleep(duration).await;
    let contest = contests.lock().unwrap().remove(&channel_id).unwrap();

    if contest.counts.is_empty() {
        announcement.delete(&ctx.http).await?;
    } else {
        if pin {
            announcement.unpin(&ctx.http).await.ok();
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

    Ok(contest)
}
