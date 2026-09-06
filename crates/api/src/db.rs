use chrono::{DateTime, Utc};
use lazyboy_contracts::{
    Bot, BrowserProfileMode, ComputerMode, ComputerState, ControlHolder, RunStatus, SandboxKind,
    computer_home_key, computer_scope_key,
};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: String,
    pub space_id: String,
}

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct ComputerRow {
    pub id: String,
    pub space_id: String,
    pub user_id: String,
    pub scope: String,
    pub scope_key: String,
    pub home_key: String,
    pub home_revision: String,
    pub kind: String,
    pub provider_ref: Option<String>,
    pub state: String,
    pub control_holder: String,
    pub control_lease_id: Option<String>,
    pub control_lease_expires_at: Option<DateTime<Utc>>,
    pub control_bot_id: Option<String>,
    pub control_run_id: Option<String>,
    pub execution_run_id: Option<String>,
    pub execution_bot_id: Option<String>,
    pub execution_lease_expires_at: Option<DateTime<Utc>>,
    pub execution_fence: i32,
    pub browser_profile_mode: String,
}

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct ScreenRow {
    pub id: String,
    pub computer_id: String,
    pub bot_id: String,
    pub slot: i32,
    pub display: String,
    pub view_port: i32,
    pub profile_mode: String,
    pub profile_path: String,
    pub control_holder: String,
    pub control_lease_id: Option<String>,
    pub control_lease_expires_at: Option<DateTime<Utc>>,
    pub execution_run_id: Option<String>,
    pub execution_lease_expires_at: Option<DateTime<Utc>>,
    pub execution_fence: i32,
}

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct BotRow {
    pub id: String,
    pub space_id: String,
    pub user_id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub avatar_color: String,
    pub avatar_shape: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub hidden: bool,
    pub group_name: Option<String>,
    pub unread_count: i64,
    pub last_message_at: Option<DateTime<Utc>>,
    pub instructions: String,
    pub computer_id: Option<String>,
    pub model_provider: Option<String>,
    pub model_id: Option<String>,
    pub memory_enabled: bool,
}

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct SpaceRow {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub default_model_provider: String,
    pub default_model_id: String,
    pub default_model_base_url: Option<String>,
    pub default_model_api_key: Option<String>,
    pub voice_enabled: bool,
    pub voice_provider: Option<String>,
    pub voice_model_id: Option<String>,
    pub voice_id: Option<String>,
    pub voice_api_key: Option<String>,
}

impl Db {
    pub async fn ensure_local_actor(&self) -> Result<Actor, sqlx::Error> {
        let user_id = "local-user";
        let space_id = "local-space";
        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(user_id)
            .bind("Local")
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "INSERT INTO spaces (id, user_id, name, is_default, default_model_provider, default_model_id)
             VALUES ($1, $2, $3, TRUE, 'xai', 'grok-4.6')
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(space_id)
        .bind(user_id)
        .bind("Home")
        .execute(&self.pool)
        .await?;
        Ok(Actor {
            user_id: user_id.into(),
            space_id: space_id.into(),
        })
    }

    pub async fn get_space(&self, actor: &Actor) -> Result<Option<SpaceRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, user_id, name, default_model_provider, default_model_id,
                    default_model_base_url, default_model_api_key,
                    voice_enabled, voice_provider, voice_model_id, voice_id, voice_api_key
             FROM spaces WHERE id = $1 AND user_id = $2",
        )
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn update_workspace_model(
        &self,
        actor: &Actor,
        provider: &str,
        model_id: &str,
        base_url: Option<&str>,
        api_key: Option<Option<&str>>,
    ) -> Result<SpaceRow, sqlx::Error> {
        match api_key {
            Some(key) => {
                sqlx::query(
                    "UPDATE spaces
                     SET default_model_provider = $3, default_model_id = $4,
                         default_model_base_url = $5, default_model_api_key = $6
                     WHERE id = $1 AND user_id = $2",
                )
                .bind(&actor.space_id)
                .bind(&actor.user_id)
                .bind(provider)
                .bind(model_id)
                .bind(base_url)
                .bind(key)
                .execute(&self.pool)
                .await?;
            }
            None => {
                sqlx::query(
                    "UPDATE spaces
                     SET default_model_provider = $3, default_model_id = $4,
                         default_model_base_url = $5
                     WHERE id = $1 AND user_id = $2",
                )
                .bind(&actor.space_id)
                .bind(&actor.user_id)
                .bind(provider)
                .bind(model_id)
                .bind(base_url)
                .execute(&self.pool)
                .await?;
            }
        }
        self.get_space(actor).await?.ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn update_voice_settings(
        &self,
        actor: &Actor,
        enabled: Option<bool>,
        provider: &str,
        model_id: &str,
        voice_id: &str,
        api_key: Option<Option<&str>>,
    ) -> Result<SpaceRow, sqlx::Error> {
        match api_key {
            Some(key) => {
                sqlx::query(
                    "UPDATE spaces
                     SET voice_provider = $3, voice_model_id = $4, voice_id = $5, voice_api_key = $6, voice_enabled = COALESCE($7, voice_enabled)
                     WHERE id = $1 AND user_id = $2",
                )
                .bind(&actor.space_id)
                .bind(&actor.user_id)
                .bind(provider)
                .bind(model_id)
                .bind(voice_id)
                .bind(key)
                .bind(enabled)
                .execute(&self.pool)
                .await?;
            }
            None => {
                sqlx::query(
                    "UPDATE spaces
                     SET voice_provider = $3, voice_model_id = $4, voice_id = $5, voice_enabled = COALESCE($6, voice_enabled)
                     WHERE id = $1 AND user_id = $2",
                )
                .bind(&actor.space_id)
                .bind(&actor.user_id)
                .bind(provider)
                .bind(model_id)
                .bind(voice_id)
                .bind(enabled)
                .execute(&self.pool)
                .await?;
            }
        }
        self.get_space(actor).await?.ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn list_bots(
        &self,
        actor: &Actor,
    ) -> Result<Vec<(BotRow, String, ComputerRow)>, sqlx::Error> {
        let bots: Vec<BotRow> = sqlx::query_as(
            "SELECT b.id, b.space_id, b.user_id, b.name, b.title, b.description, b.avatar_color, b.avatar_shape, b.tags,
                    b.pinned, b.hidden, b.group_name,
                    (SELECT COUNT(*) FROM messages m JOIN threads t ON t.id=m.thread_id
                     WHERE t.bot_id=b.id AND t.room_id IS NULL AND m.role='assistant' AND m.created_at>b.last_read_at) AS unread_count,
                    (SELECT MAX(m.created_at) FROM messages m JOIN threads t ON t.id=m.thread_id WHERE t.bot_id=b.id AND t.room_id IS NULL) AS last_message_at,
                    b.instructions, b.computer_id, b.model_provider, b.model_id, b.memory_enabled
             FROM bots b WHERE b.space_id = $1 AND b.user_id = $2 ORDER BY b.pinned DESC, b.created_at DESC",
        )
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for bot in bots {
            let thread_id: (String,) = sqlx::query_as(
                "SELECT id FROM threads
                     WHERE bot_id = $1 AND space_id = $2 AND user_id = $3 AND room_id IS NULL
                     ORDER BY updated_at DESC, created_at ASC LIMIT 1",
            )
            .bind(&bot.id)
            .bind(&actor.space_id)
            .bind(&actor.user_id)
            .fetch_one(&self.pool)
            .await?;
            let computer = self
                .get_computer(bot.computer_id.as_deref().unwrap_or(""))
                .await?;
            if let Some(computer) = computer {
                out.push((bot, thread_id.0, computer));
            }
        }
        Ok(out)
    }

    pub async fn get_bot(
        &self,
        actor: &Actor,
        bot_id: &str,
    ) -> Result<Option<BotRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT b.id, b.space_id, b.user_id, b.name, b.title, b.description, b.avatar_color, b.avatar_shape, b.tags,
                    b.pinned, b.hidden, b.group_name,
                    (SELECT COUNT(*) FROM messages m JOIN threads t ON t.id=m.thread_id
                     WHERE t.bot_id=b.id AND t.room_id IS NULL AND m.role='assistant' AND m.created_at>b.last_read_at) AS unread_count,
                    (SELECT MAX(m.created_at) FROM messages m JOIN threads t ON t.id=m.thread_id WHERE t.bot_id=b.id AND t.room_id IS NULL) AS last_message_at,
                    b.instructions, b.computer_id, b.model_provider, b.model_id, b.memory_enabled
             FROM bots b WHERE b.id = $1 AND b.space_id = $2 AND b.user_id = $3",
        )
        .bind(bot_id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn get_computer(
        &self,
        computer_id: &str,
    ) -> Result<Option<ComputerRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, space_id, user_id, scope, scope_key, home_key, home_revision, kind, provider_ref, state,
                    control_holder, control_lease_id, control_lease_expires_at, control_bot_id, control_run_id,
                    execution_run_id, execution_bot_id, execution_lease_expires_at, execution_fence,
                    browser_profile_mode
             FROM computers WHERE id = $1",
        )
        .bind(computer_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn create_bot(
        &self,
        actor: &Actor,
        name: &str,
        title: &str,
        description: &str,
        instructions: &str,
        mode: ComputerMode,
        model_provider: Option<&str>,
        model_id: Option<&str>,
        memory_enabled: bool,
    ) -> Result<Bot, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let bot_id = Uuid::new_v4().to_string();
        let thread_id = Uuid::new_v4().to_string();
        let computer = ensure_computer(
            &mut tx,
            actor,
            mode,
            (mode == ComputerMode::Dedicated).then_some(bot_id.as_str()),
        )
        .await?;
        sqlx::query(
            "INSERT INTO bots (id, space_id, user_id, name, title, description, instructions, computer_id, model_provider, model_id, memory_enabled)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(&bot_id)
        .bind(&actor.space_id)
        .bind(&actor.user_id)
        .bind(name)
        .bind(title)
        .bind(description)
        .bind(instructions)
        .bind(&computer.id)
        .bind(model_provider)
        .bind(model_id)
        .bind(memory_enabled)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO threads (id, space_id, bot_id, user_id, title) VALUES ($1,$2,$3,$4,'新對話')")
            .bind(&thread_id)
            .bind(&actor.space_id)
            .bind(&bot_id)
            .bind(&actor.user_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Bot {
            id: bot_id,
            space_id: actor.space_id.clone(),
            name: name.into(),
            title: title.into(),
            description: description.into(),
            avatar_color: "#8B5CF6".into(),
            avatar_shape: "blob".into(),
            tags: Vec::new(),
            pinned: false,
            hidden: false,
            group_name: None,
            unread_count: 0,
            last_message_at: None,
            instructions: instructions.into(),
            thread_id,
            computer_id: computer.id,
            computer_mode: mode,
            model_provider: model_provider.and_then(|value| value.parse().ok()),
            model_id: model_id.map(str::to_string),
            memory_enabled,
        })
    }

    pub async fn get_screen(
        &self,
        computer_id: &str,
        bot_id: &str,
    ) -> Result<Option<ScreenRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, computer_id, bot_id, slot, display, view_port, profile_mode, profile_path,
                    control_holder, control_lease_id, control_lease_expires_at, execution_run_id,
                    execution_lease_expires_at, execution_fence
             FROM computer_screens WHERE computer_id = $1 AND bot_id = $2",
        )
        .bind(computer_id)
        .bind(bot_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn list_screen_slots(&self, computer_id: &str) -> Result<Vec<i32>, sqlx::Error> {
        sqlx::query_scalar("SELECT slot FROM computer_screens WHERE computer_id = $1 ORDER BY slot")
            .bind(computer_id)
            .fetch_all(&self.pool)
            .await
    }

    pub async fn list_screens(&self, computer_id: &str) -> Result<Vec<ScreenRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, computer_id, bot_id, slot, display, view_port, profile_mode, profile_path,
                    control_holder, control_lease_id, control_lease_expires_at, execution_run_id,
                    execution_lease_expires_at, execution_fence
             FROM computer_screens WHERE computer_id = $1 ORDER BY slot",
        )
        .bind(computer_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn active_run(
        &self,
        bot_id: &str,
    ) -> Result<Option<(String, String, String)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, status, thread_id FROM runs
             WHERE bot_id = $1
               AND status IN ('queued','leased','running','waiting_input','waiting_takeover')
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(bot_id)
        .fetch_optional(&self.pool)
        .await
    }
}

async fn ensure_computer(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &Actor,
    mode: ComputerMode,
    bot_id: Option<&str>,
) -> Result<ComputerRow, sqlx::Error> {
    let scope_key = computer_scope_key(mode, &actor.space_id, bot_id).expect("scope key");
    let home_key = computer_home_key(mode, &actor.space_id, bot_id).expect("home key");
    sqlx::query(
        "INSERT INTO computers (id, space_id, user_id, scope, scope_key, home_key, kind, state)
         VALUES ($1,$2,$3,$4,$5,$6,'docker','stopped')
         ON CONFLICT (scope_key) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&actor.space_id)
    .bind(&actor.user_id)
    .bind(mode.as_str())
    .bind(&scope_key)
    .bind(&home_key)
    .execute(&mut **tx)
    .await?;
    sqlx::query_as(
        "SELECT id, space_id, user_id, scope, scope_key, home_key, home_revision, kind, provider_ref, state,
                control_holder, control_lease_id, control_lease_expires_at, control_bot_id, control_run_id,
                execution_run_id, execution_bot_id, execution_lease_expires_at, execution_fence,
                browser_profile_mode
         FROM computers WHERE scope_key = $1",
    )
    .bind(scope_key)
    .fetch_one(&mut **tx)
    .await
}

pub fn parse_mode(scope: &str) -> ComputerMode {
    scope.parse().unwrap_or(ComputerMode::Team)
}

pub fn parse_state(state: &str) -> ComputerState {
    match state {
        "booting" => ComputerState::Booting,
        "running" => ComputerState::Running,
        "suspended" => ComputerState::Suspended,
        "error" => ComputerState::Error,
        _ => ComputerState::Stopped,
    }
}

pub fn parse_holder(holder: &str) -> ControlHolder {
    match holder {
        "bot" => ControlHolder::Bot,
        "user" => ControlHolder::User,
        _ => ControlHolder::None,
    }
}

pub fn parse_kind(kind: &str) -> SandboxKind {
    let _ = kind;
    SandboxKind::Docker
}

pub fn parse_profile_mode(value: &str) -> BrowserProfileMode {
    value.parse().unwrap_or(BrowserProfileMode::PerBot)
}

pub fn parse_run_status(status: &str) -> Option<RunStatus> {
    match status {
        "queued" => Some(RunStatus::Queued),
        "leased" => Some(RunStatus::Leased),
        "running" => Some(RunStatus::Running),
        "waiting_input" => Some(RunStatus::WaitingInput),
        "waiting_takeover" => Some(RunStatus::WaitingTakeover),
        "completed" => Some(RunStatus::Completed),
        "failed" => Some(RunStatus::Failed),
        "cancelled" => Some(RunStatus::Cancelled),
        _ => None,
    }
}
