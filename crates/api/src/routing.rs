//! Who is allowed to answer a group message.
//!
//! A room used to wake every member for every message, so a group of four paid
//! for four model calls (and four desktop claims) to get one useful answer. The
//! audience is now decided once, when the message arrives: an `@name` is law,
//! and without one a single short model call picks the members whose role fits
//! the ask. When nothing fits, the host answers — a message is never left with
//! nobody to read it, because a silently dropped message is worse than one
//! wasted reply.

use std::time::Duration;

use lazyboy_contracts::ModelProvider;
use lazyboy_harness::{
    CredentialChain, DynModel, ResolveModelRequest, connect_model, credential_from_env,
    resolve_backend,
};
use rig_core::completion::message::{AssistantContent, Message, UserContent};

use crate::db::Actor;
use crate::state::AppState;

/// The picker is a nicety, not a gate: past this the host takes over, so a slow
/// helper never holds up a chat message.
const ROUTER_TIMEOUT: Duration = Duration::from_millis(1200);
/// How many members one unprefixed message may wake.
const ROUTER_CAP: usize = 3;
/// Recent lines and role text handed to the picker.
const ROUTER_HISTORY: i64 = 6;
const FIELD_CHARS: usize = 100;
const LINE_CHARS: usize = 200;
/// `@所有人` and its spellings: everybody, and no model call at all. The web
/// composer offers the escape hatch in whatever language the screen is in, so
/// every spelling it can insert has to be understood here too.
const ALL_KEYWORDS: [&str; 5] = ["所有人", "全部", "all", "everyone", "everybody"];

const PICKER_SYSTEM: &str = "You decide which members of a group chat should answer the newest message.

Reply with names from the roster only, separated by 、, at most three of them. Choose a member when the message falls inside what that member does. Reply NONE when no roster member fits, and add nothing else.
Never invent a name, never explain your choice, and never answer the message yourself.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub name: String,
    /// What this member says it does (title + description). Picker input only;
    /// empty is fine.
    pub role: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mentions {
    /// `@所有人` appeared.
    pub all: bool,
    /// Member ids in the order they were named.
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    All,
    Mention,
    Routed,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audience {
    pub targets: Vec<String>,
    pub reason: Reason,
}

/// A room reduced to what routing needs: who leads it and who is in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub id: String,
    pub host_id: String,
    pub members: Vec<Member>,
}

/// Names written as `@名字`, longest name first so a short member cannot steal a
/// longer member's mention.
pub fn parse_mentions(text: &str, members: &[Member]) -> Mentions {
    let mut mentions = Mentions::default();
    let chars: Vec<char> = text.chars().collect();
    for (position, ch) in chars.iter().enumerate() {
        // `mail@example.com` is not a mention: `@` has to open a word.
        if *ch != '@' || (position > 0 && chars[position - 1].is_alphanumeric()) {
            continue;
        }
        let tail: String = chars[position + 1..].iter().collect();
        if ALL_KEYWORDS.iter().any(|word| name_hit(&tail, word)) {
            mentions.all = true;
            continue;
        }
        if let Some(id) = find_names(&tail, members, true).into_iter().next()
            && !mentions.ids.contains(&id)
        {
            mentions.ids.push(id);
        }
    }
    mentions
}

/// Names of roster members that appear in `text`, in order of appearance.
/// `anchored` accepts a name only at the very start of `text`, which is what a
/// mention needs after the `@`.
fn find_names(text: &str, members: &[Member], anchored: bool) -> Vec<String> {
    let mut ranked: Vec<&Member> = members
        .iter()
        .filter(|member| !member.name.trim().is_empty())
        .collect();
    ranked.sort_by_key(|member| std::cmp::Reverse(member.name.chars().count()));
    let chars: Vec<char> = text.chars().collect();
    let mut ids: Vec<String> = Vec::new();
    let mut position = 0usize;
    while position < chars.len() {
        let tail: String = chars[position..].iter().collect();
        if let Some(member) = ranked.iter().find(|member| name_hit(&tail, &member.name))
            && !ids.contains(&member.id)
        {
            ids.push(member.id.clone());
        }
        if anchored {
            break;
        }
        position += 1;
    }
    ids
}

/// Does `tail` open with `name`? ASCII names need a word boundary after them so
/// `@Ali` cannot pick "Alice"; CJK names run together, so the longest match is
/// already the boundary.
fn name_hit(tail: &str, name: &str) -> bool {
    let lowered = tail.to_lowercase();
    let needle = name.trim().to_lowercase();
    if needle.is_empty() || !lowered.starts_with(&needle) {
        return false;
    }
    if !needle
        .chars()
        .next_back()
        .is_some_and(|last| last.is_ascii_alphanumeric())
    {
        return true;
    }
    match lowered.chars().nth(needle.chars().count()) {
        None => true,
        Some(next) => !(next.is_ascii_alphanumeric() || next == '-' || next == '_'),
    }
}

/// The picker's answer reduced to roster ids. It may write "阿明、小美", a JSON
/// array, or prose naming people; anything outside the roster is dropped.
pub fn parse_router_reply(raw: &str, members: &[Member]) -> Vec<String> {
    let mut picked = find_names(raw, members, false);
    picked.truncate(ROUTER_CAP);
    picked
}

/// Who should run for one message: a name is law, `@所有人` is everybody, and
/// only an unaddressed message consults the picker.
pub fn resolve_audience(
    members: &[Member],
    mentions: &Mentions,
    host_id: &str,
    busy: &[String],
    routed: &[String],
) -> Audience {
    let roster: Vec<&String> = members.iter().map(|member| &member.id).collect();
    if mentions.all {
        return Audience {
            targets: roster.into_iter().cloned().collect(),
            reason: Reason::All,
        };
    }
    let named: Vec<String> = mentions
        .ids
        .iter()
        .filter(|id| roster.contains(id))
        .cloned()
        .collect();
    if !named.is_empty() {
        return Audience {
            targets: named,
            reason: Reason::Mention,
        };
    }
    let chosen: Vec<String> = routed
        .iter()
        .filter(|id| roster.contains(id) && !busy.contains(id))
        .cloned()
        .collect();
    if !chosen.is_empty() {
        return Audience {
            targets: chosen,
            reason: Reason::Routed,
        };
    }
    // Never zero: the host picks up what nobody was chosen for.
    Audience {
        targets: vec![host_id.to_string()],
        reason: Reason::Host,
    }
}

/// The room a thread belongs to, or `None` for a one-to-one conversation.
pub(crate) async fn room_for_thread(
    state: &AppState,
    thread_id: &str,
) -> Result<Option<Room>, String> {
    // The column is nullable and the row may be missing: both read as "not a
    // room", so the outer `Option` (no row) and the inner one (NULL) are
    // flattened together. Decoding the column straight into `String` made every
    // one-to-one message fail with an "unexpected null".
    let room_id: Option<Option<String>> =
        sqlx::query_scalar("SELECT room_id FROM threads WHERE id=$1")
            .bind(thread_id)
            .fetch_optional(state.pool())
            .await
            .map_err(|error| error.to_string())?;
    let Some(room_id) = room_id.flatten() else {
        return Ok(None);
    };
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT b.id, b.name, b.title, b.description
         FROM room_members m JOIN bots b ON b.id=m.bot_id
         WHERE m.room_id=$1
         ORDER BY m.created_at, b.name",
    )
    .bind(&room_id)
    .fetch_all(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    if rows.is_empty() {
        return Ok(None);
    }
    let members: Vec<Member> = rows
        .into_iter()
        .map(|(id, name, title, description)| Member {
            role: format!("{title} {description}").trim().to_string(),
            id,
            name,
        })
        .collect();
    let host: Option<Option<String>> =
        sqlx::query_scalar("SELECT host_bot_id FROM rooms WHERE id=$1")
            .bind(&room_id)
            .fetch_optional(state.pool())
            .await
            .map_err(|error| error.to_string())?;
    // A deleted host, or a room written before this column existed, falls back
    // to the first member — who is the one that used to answer anyway.
    let host_id = host
        .flatten()
        .filter(|host| members.iter().any(|member| member.id == *host))
        .unwrap_or_else(|| members[0].id.clone());
    Ok(Some(Room {
        id: room_id,
        host_id,
        members,
    }))
}

/// Members of `room` that already have a run of their own in flight.
pub(crate) async fn busy_members(state: &AppState, room: &Room) -> Result<Vec<String>, String> {
    let roster: Vec<String> = room
        .members
        .iter()
        .map(|member| member.id.clone())
        .collect();
    let busy: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT bot_id FROM runs
         WHERE bot_id = ANY($1)
           AND status IN ('queued','leased','running','waiting_input','waiting_takeover')",
    )
    .bind(&roster)
    .fetch_all(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    Ok(busy)
}

/// Decide the audience for a message that has not been written yet. The model
/// call happens outside the message transaction, so a slow picker never holds a
/// row lock, and every failure path lands on the host.
pub(crate) async fn audience_for(
    state: &AppState,
    actor: &Actor,
    room: &Room,
    thread_id: &str,
    text: &str,
) -> Audience {
    let mentions = parse_mentions(text, &room.members);
    let decided = resolve_audience(&room.members, &mentions, &room.host_id, &[], &[]);
    if decided.reason != Reason::Host {
        log(room, thread_id, &decided);
        return decided;
    }
    let busy = busy_members(state, room).await.unwrap_or_default();
    // Two members need no referee: the host answers, and naming the other one
    // is free.
    let routed = if room.members.len() > 2 {
        pick(state, actor, room, thread_id, text).await
    } else {
        Vec::new()
    };
    let decided = resolve_audience(&room.members, &mentions, &room.host_id, &busy, &routed);
    log(room, thread_id, &decided);
    decided
}

fn log(room: &Room, thread_id: &str, audience: &Audience) {
    tracing::info!(
        room = room.id,
        thread = thread_id,
        reason = reason_label(audience.reason),
        targets = ?audience.targets,
        "room routing"
    );
}

pub fn reason_label(reason: Reason) -> &'static str {
    match reason {
        Reason::All => "all",
        Reason::Mention => "mention",
        Reason::Routed => "routed",
        Reason::Host => "host",
    }
}

/// One short model call: which names fit this message?
async fn pick(
    state: &AppState,
    actor: &Actor,
    room: &Room,
    thread_id: &str,
    text: &str,
) -> Vec<String> {
    let model = match router_model(state, actor).await {
        Ok(model) => model,
        Err(error) => {
            tracing::warn!("room router unavailable: {error}");
            return Vec::new();
        }
    };
    let prompt = match prompt_for(state, room, thread_id, text).await {
        Ok(prompt) => prompt,
        Err(error) => {
            tracing::warn!("room router prompt: {error}");
            return Vec::new();
        }
    };
    let attempt = tokio::time::timeout(
        ROUTER_TIMEOUT,
        crate::runs::complete_once(
            &model,
            Message::User {
                content: vec![UserContent::text(prompt)],
            },
            PICKER_SYSTEM,
            &[],
            &[],
        ),
    )
    .await;
    let raw = match attempt {
        Ok(Ok(parts)) => parts
            .into_iter()
            .filter_map(|part| match part {
                AssistantContent::Text(text) => Some(text.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        Ok(Err(error)) => {
            tracing::warn!("room router call: {error}");
            return Vec::new();
        }
        Err(_) => {
            tracing::warn!("room router timed out; the host answers");
            return Vec::new();
        }
    };
    parse_router_reply(&raw, &room.members)
}

async fn prompt_for(
    state: &AppState,
    room: &Room,
    thread_id: &str,
    text: &str,
) -> Result<String, String> {
    let recent: Vec<(String, String)> = sqlx::query_as(
        "SELECT COALESCE(b.name, '你') AS speaker, m.body
         FROM messages m LEFT JOIN bots b ON b.id=m.speaker_bot_id
         WHERE m.thread_id=$1
         ORDER BY m.seq DESC LIMIT $2",
    )
    .bind(thread_id)
    .bind(ROUTER_HISTORY)
    .fetch_all(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    let mut prompt = String::from("Roster (name — what they do):\n");
    for member in &room.members {
        prompt.push_str(&format!(
            "- {} — {}\n",
            clip(&member.name, FIELD_CHARS),
            if member.role.is_empty() {
                "—".to_string()
            } else {
                clip(&member.role, FIELD_CHARS)
            }
        ));
    }
    if !recent.is_empty() {
        prompt.push_str("\nRecent lines (newest last):\n");
        for (speaker, body) in recent.iter().rev() {
            prompt.push_str(&format!(
                "{}: {}\n",
                clip(speaker, 40),
                clip(body, LINE_CHARS)
            ));
        }
    }
    prompt.push_str("\nNewest message:\n");
    prompt.push_str(text.trim());
    prompt.push_str("\n\nNames that should answer:");
    Ok(prompt)
}

/// The picker runs on the workspace model unless `LAZYBOY_ROUTER_MODEL` names a
/// cheaper one; leaving it unset is a valid and common state.
async fn router_model(state: &AppState, actor: &Actor) -> Result<DynModel, String> {
    let space = state
        .db
        .get_space(actor)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "workspace not found".to_string())?;
    let provider = space
        .default_model_provider
        .parse::<ModelProvider>()
        .map_err(|error| error.to_string())?;
    let override_id = std::env::var("LAZYBOY_ROUTER_MODEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let backend = resolve_backend(ResolveModelRequest {
        provider,
        model_id: override_id.or(Some(space.default_model_id)),
        base_url: space.default_model_base_url.clone(),
        credentials: CredentialChain {
            bot: None,
            space: space.default_model_api_key.clone(),
            env: credential_from_env(provider),
        },
    })
    .map_err(|error| error.to_string())?;
    connect_model(&backend).map_err(|error| error.to_string())
}

/// The single hand-off a bot may pass on: a member it names that is neither
/// itself nor already working. `@所有人` from a bot is ignored on purpose — a
/// bot must never broadcast the room.
pub(crate) async fn handoff_target(
    state: &AppState,
    run_id: &str,
    from_bot_id: &str,
    body: &str,
) -> Result<Option<String>, String> {
    let producing: Option<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT r.thread_id, t.room_id, r.\"trigger\"
         FROM runs r JOIN threads t ON t.id=r.thread_id WHERE r.id=$1",
    )
    .bind(run_id)
    .fetch_optional(state.pool())
    .await
    .map_err(|error| error.to_string())?;
    let Some((thread_id, Some(_room_id), trigger)) = producing else {
        return Ok(None);
    };
    // One hop, hard stop: a run that exists because of a hand-off cannot start
    // another one, or two chatty bots would keep the room (and the bill) alive.
    if trigger == "handoff" {
        return Ok(None);
    }
    let Some(room) = room_for_thread(state, &thread_id).await? else {
        return Ok(None);
    };
    let mentions = parse_mentions(body, &room.members);
    if mentions.all || mentions.ids.is_empty() {
        return Ok(None);
    }
    let busy = busy_members(state, &room).await.unwrap_or_default();
    Ok(mentions
        .ids
        .into_iter()
        .find(|id| id != from_bot_id && !busy.contains(id)))
}

/// Trim to whole characters so a clipped role or line never splits in half.
fn clip(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, name: &str) -> Member {
        Member {
            id: id.to_string(),
            name: name.to_string(),
            role: String::new(),
        }
    }

    fn roster() -> Vec<Member> {
        vec![member("a", "阿明"), member("b", "小美"), member("c", "Dev")]
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// Ids in the order they were found. Anything outside the roster is quoted,
    /// so a failure points at the stranger rather than looking like a member.
    fn found(picked: &[String], room: &[Member]) -> Vec<String> {
        picked
            .iter()
            .map(|id| {
                if room.iter().any(|member| member.id == *id) {
                    id.clone()
                } else {
                    format!("«{id}»")
                }
            })
            .collect()
    }

    #[test]
    fn only_a_named_member_is_consulted() {
        let room = roster();
        let mentions = parse_mentions("@小美 幫我看這份報表", &room);
        assert!(!mentions.all);
        assert_eq!(found(&mentions.ids, &room), ["b"]);
    }

    #[test]
    fn the_longest_name_wins_when_members_overlap() {
        let room = vec![member("a", "阿明"), member("b", "阿明哥")];
        assert_eq!(
            found(&parse_mentions("@阿明哥 來一下", &room).ids, &room),
            ["b"]
        );
        assert_eq!(
            found(&parse_mentions("@阿明 來一下", &room).ids, &room),
            ["a"]
        );
    }

    #[test]
    fn ascii_names_need_a_word_boundary_after_them() {
        let room = roster();
        assert_eq!(
            found(&parse_mentions("@dev 幫我看", &room).ids, &room),
            ["c"]
        );
        assert_eq!(
            found(&parse_mentions("@Dev, help", &room).ids, &room),
            ["c"]
        );
        assert!(parse_mentions("@Developer 幫我看", &room).ids.is_empty());
    }

    #[test]
    fn punctuation_does_not_eat_a_mention() {
        let room = roster();
        assert_eq!(
            found(&parse_mentions("@小美，@阿明 一起看", &room).ids, &room),
            ["b", "a"]
        );
        assert_eq!(found(&parse_mentions("（@小美）", &room).ids, &room), ["b"]);
    }

    #[test]
    fn an_email_address_is_not_a_mention() {
        let room = roster();
        let mentions = parse_mentions("寄到 mail@小美 或 report@dev.example 就好", &room);
        assert!(mentions.ids.is_empty() && !mentions.all);
    }

    #[test]
    fn naming_everyone_is_the_escape_hatch() {
        let room = roster();
        for text in [
            "@所有人 結論定了",
            "@全部 來看",
            "@all look",
            "@everyone look",
            "@everybody, your turn",
        ] {
            assert!(parse_mentions(text, &room).all, "{text}");
        }
        // `@allison` is somebody's name, not `all`.
        assert!(!parse_mentions("@allison 來了", &room).all);
    }

    #[test]
    fn unknown_names_are_dropped_and_repeats_count_once() {
        let room = roster();
        let mentions = parse_mentions("@阿明 和 @路人，@阿明 再說一次", &room);
        assert_eq!(found(&mentions.ids, &room), ["a"]);
    }

    #[test]
    fn the_picker_may_answer_in_names_json_or_prose() {
        let room = roster();
        assert_eq!(
            found(&parse_router_reply("小美、阿明", &room), &room),
            ["b", "a"]
        );
        assert_eq!(
            found(&parse_router_reply("[\"小美\", \"Dev\"]", &room), &room),
            ["b", "c"]
        );
        assert_eq!(
            found(&parse_router_reply("我覺得小美可以回答", &room), &room),
            ["b"]
        );
    }

    #[test]
    fn the_picker_cannot_invent_members_or_a_crowd() {
        let room = roster();
        assert!(parse_router_reply("NONE", &room).is_empty());
        assert!(parse_router_reply("請由 Grace 處理", &room).is_empty());
        assert!(parse_router_reply("Development work", &room).is_empty());
        let crowd = vec![
            member("a", "一"),
            member("b", "二"),
            member("c", "三"),
            member("d", "四"),
        ];
        assert_eq!(
            parse_router_reply("一、二、三、四", &crowd).len(),
            ROUTER_CAP
        );
    }

    #[test]
    fn a_named_member_answers_even_while_the_picker_skips_busy_ones() {
        let room = roster();
        let busy = ids(&["b"]);
        let mentioned = resolve_audience(
            &room,
            &Mentions {
                all: false,
                ids: ids(&["b"]),
            },
            "a",
            &busy,
            &[],
        );
        assert_eq!(mentioned.targets, ids(&["b"]));
        assert_eq!(mentioned.reason, Reason::Mention);

        let routed = resolve_audience(&room, &Mentions::default(), "a", &busy, &ids(&["b", "c"]));
        assert_eq!(routed.targets, ids(&["c"]));
        assert_eq!(routed.reason, Reason::Routed);
    }

    #[test]
    fn everybody_or_nobody_is_settled_without_the_picker() {
        let room = roster();
        let all = resolve_audience(
            &room,
            &Mentions {
                all: true,
                ids: Vec::new(),
            },
            "a",
            &[],
            &[],
        );
        assert_eq!(all.targets, ids(&["a", "b", "c"]));
        assert_eq!(all.reason, Reason::All);

        // Nothing fitted, the picker named strangers, or it named only busy
        // members: the host picks it up rather than the message going unread.
        let stranded: Vec<Vec<String>> = vec![vec![], ids(&["ghost"]), ids(&["b"])];
        for routed in stranded {
            let host = resolve_audience(&room, &Mentions::default(), "a", &ids(&["b"]), &routed);
            assert_eq!(host.targets, ids(&["a"]));
            assert_eq!(host.reason, Reason::Host);
        }
    }

    #[test]
    fn an_id_that_left_the_room_is_not_an_audience() {
        let room = roster();
        let audience = resolve_audience(
            &room,
            &Mentions {
                all: false,
                ids: ids(&["ghost"]),
            },
            "a",
            &[],
            &ids(&["c"]),
        );
        assert_eq!(audience.targets, ids(&["c"]));
        assert_eq!(audience.reason, Reason::Routed);
    }

    #[test]
    fn clipping_counts_characters_not_bytes() {
        assert_eq!(clip("群組聊天", 3), "群組聊");
    }
}

/// The same decisions with the real tables behind them: how many runs one chat
/// message is allowed to start, and where a hand-off stops.
#[cfg(test)]
mod fan_out {
    use super::*;
    use serde_json::json;

    fn app(pool: sqlx::PgPool) -> AppState {
        AppState {
            db: crate::db::Db { pool },
            sandbox: std::sync::Arc::new(lazyboy_sandbox::FakeSandbox::new()),
            data_dir: String::new(),
            auth: crate::auth::AuthConfig::from_env(),
            memory: crate::memory::MemoryService::from_env(),
            mcp: crate::mcp::McpHub::new(),
            calls: crate::state::CallRegistry::default(),
            wakes: crate::state::WakeBus::default(),
        }
    }

    /// `members` join one room led by `host`, and share the thread `t`. A
    /// `group` of one is a direct message, which has no audience to choose.
    async fn seed(pool: &sqlx::PgPool, members: &[(&str, &str)], host: Option<&str>, group: bool) {
        sqlx::query("INSERT INTO users (id,name) VALUES ('u','test')")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO spaces (id,user_id,name) VALUES ('s','u','test')")
            .execute(pool)
            .await
            .unwrap();
        for (id, name) in members {
            sqlx::query("INSERT INTO bots (id,space_id,user_id,name) VALUES ($1,'s','u',$2)")
                .bind(id)
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
        if group {
            sqlx::query(
                "INSERT INTO rooms (id,space_id,user_id,name,host_bot_id)
                 VALUES ('r','s','u','產品群',$1)",
            )
            .bind(host)
            .execute(pool)
            .await
            .unwrap();
            for (id, _) in members {
                sqlx::query("INSERT INTO room_members (room_id,bot_id) VALUES ('r',$1)")
                    .bind(id)
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
        sqlx::query(
            "INSERT INTO threads (id,space_id,user_id,bot_id,room_id) VALUES ('t','s','u',$1,$2)",
        )
        .bind(members[0].0)
        .bind(group.then_some("r"))
        .execute(pool)
        .await
        .unwrap();
    }

    async fn start_run(pool: &sqlx::PgPool, id: &str, bot: &str, trigger: &str) {
        sqlx::query(
            "INSERT INTO runs (id,space_id,user_id,bot_id,thread_id,status,trigger,prompt)
             VALUES ($1,'s','u',$2,'t','running',$3,'開始')",
        )
        .bind(id)
        .bind(bot)
        .bind(trigger)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn runs_for(pool: &sqlx::PgPool, bot: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM runs WHERE bot_id=$1")
            .bind(bot)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// The audience written next to the newest message of `role`. Empty means
    /// nothing was recorded, which is what a direct message must do.
    async fn audience_of(pool: &sqlx::PgPool, role: &str) -> Vec<String> {
        let stored: Option<Option<Vec<String>>> =
            sqlx::query_scalar("SELECT reply_bot_ids FROM messages WHERE role=$1 LIMIT 1")
                .bind(role)
                .fetch_optional(pool)
                .await
                .unwrap();
        stored.flatten().unwrap_or_default()
    }

    const TRIO: [(&str, &str); 3] = [("a", "阿明"), ("b", "小美"), ("c", "阿強")];

    #[sqlx::test(migrations = "../../migrations")]
    async fn only_the_member_named_in_the_message_is_woken(pool: sqlx::PgPool) {
        seed(&pool, &TRIO, Some("a"), true).await;
        let actor = Actor {
            user_id: "u".into(),
            space_id: "s".into(),
        };
        crate::runs::send(
            &app(pool.clone()),
            &actor,
            "a",
            "t",
            "@小美 幫我看這份",
            None,
            &[],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(runs_for(&pool, "b").await, 1);
        assert_eq!(runs_for(&pool, "a").await, 0);
        assert_eq!(runs_for(&pool, "c").await, 0);
        assert_eq!(audience_of(&pool, "user").await, ["b"]);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_two_member_room_answers_without_calling_the_picker(pool: sqlx::PgPool) {
        seed(&pool, &[("a", "阿明"), ("b", "小美")], Some("a"), true).await;
        let actor = Actor {
            user_id: "u".into(),
            space_id: "s".into(),
        };
        crate::runs::send(
            &app(pool.clone()),
            &actor,
            "a",
            "t",
            "誰幫我看這份",
            None,
            &[],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(runs_for(&pool, "a").await, 1);
        assert_eq!(runs_for(&pool, "b").await, 0);
        assert_eq!(audience_of(&pool, "user").await, ["a"]);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_mention_is_queued_behind_the_work_already_running(pool: sqlx::PgPool) {
        seed(&pool, &TRIO, Some("a"), true).await;
        start_run(&pool, "busy", "b", "message").await;
        let actor = Actor {
            user_id: "u".into(),
            space_id: "s".into(),
        };
        crate::runs::send(
            &app(pool.clone()),
            &actor,
            "a",
            "t",
            "@小美 再看一次",
            None,
            &[],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(runs_for(&pool, "b").await, 2);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_bot_hands_one_item_over_and_stops_there(pool: sqlx::PgPool) {
        seed(&pool, &TRIO, Some("a"), true).await;
        start_run(&pool, "r1", "a", "message").await;
        let app = app(pool.clone());
        crate::runs::append_bot_message_with(
            &app,
            "t",
            "r1",
            "a",
            "這塊我不熟，@小美 交給妳。",
            json!([]),
        )
        .await
        .unwrap();
        let passed: Vec<(String, String)> =
            sqlx::query_as("SELECT bot_id, \"trigger\" FROM runs WHERE id<>'r1'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(passed, vec![("b".to_string(), "handoff".to_string())]);
        assert_eq!(audience_of(&pool, "assistant").await, ["b"]);

        // The hand-off's own reply starts nothing: one hop is the whole budget,
        // or two chatty bots would keep the room and the bill alive.
        let handed_run: String = sqlx::query_scalar("SELECT id FROM runs WHERE id<>'r1'")
            .fetch_one(&pool)
            .await
            .unwrap();
        crate::runs::append_bot_message_with(
            &app,
            "t",
            &handed_run,
            "b",
            "@阿強 換妳。",
            json!([]),
        )
        .await
        .unwrap();
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(total, 2);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_busy_member_is_not_handed_more_work(pool: sqlx::PgPool) {
        seed(&pool, &TRIO, Some("a"), true).await;
        start_run(&pool, "busy", "b", "message").await;
        start_run(&pool, "r1", "a", "message").await;
        crate::runs::append_bot_message_with(
            &app(pool.clone()),
            "t",
            "r1",
            "a",
            "@小美 或 @阿強，誰有空？",
            json!([]),
        )
        .await
        .unwrap();
        let passed: Vec<String> =
            sqlx::query_scalar("SELECT bot_id FROM runs WHERE \"trigger\"='handoff'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(passed, vec!["c".to_string()]);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_bot_naming_everyone_hands_over_nothing(pool: sqlx::PgPool) {
        seed(&pool, &TRIO, Some("a"), true).await;
        start_run(&pool, "r1", "a", "message").await;
        crate::runs::append_bot_message_with(
            &app(pool.clone()),
            "t",
            "r1",
            "a",
            "@所有人 一起看這份。",
            json!([]),
        )
        .await
        .unwrap();
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(total, 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_direct_message_never_broadcasts(pool: sqlx::PgPool) {
        seed(&pool, &[("a", "阿明"), ("b", "小美")], None, false).await;
        start_run(&pool, "r1", "a", "message").await;
        crate::runs::append_bot_message_with(
            &app(pool.clone()),
            "t",
            "r1",
            "a",
            "@小美 @所有人 一起看。",
            json!([]),
        )
        .await
        .unwrap();
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(total, 1);
        assert!(audience_of(&pool, "assistant").await.is_empty());
    }

    /// A one-to-one thread has no room to look up; the send must simply reach
    /// its bot. This once failed before the message was even written, because a
    /// NULL `room_id` was decoded as if it could not be NULL.
    #[sqlx::test(migrations = "../../migrations")]
    async fn a_direct_message_reaches_its_bot_and_records_no_audience(pool: sqlx::PgPool) {
        seed(&pool, &[("a", "阿明")], None, false).await;
        let actor = Actor {
            user_id: "u".into(),
            space_id: "s".into(),
        };
        crate::runs::send(
            &app(pool.clone()),
            &actor,
            "a",
            "t",
            "幫我看這份",
            None,
            &[],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(runs_for(&pool, "a").await, 1);
        assert!(audience_of(&pool, "user").await.is_empty());
    }

    /// A room whose host was deleted (or that predates the column) still has a
    /// host: the first member, as before.
    #[sqlx::test(migrations = "../../migrations")]
    async fn a_room_without_a_recorded_host_falls_back_to_its_first_member(pool: sqlx::PgPool) {
        seed(&pool, &[("a", "阿明"), ("b", "小美")], None, true).await;
        let room = room_for_thread(&app(pool.clone()), "t")
            .await
            .unwrap()
            .expect("the thread belongs to a room");
        assert_eq!(room.host_id, "a");
    }
}
