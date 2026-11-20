use eframe::egui;
use egui::{Color32, RichText};

const BG: Color32 = Color32::from_rgb(16, 17, 20);
const PANEL: Color32 = Color32::from_rgb(26, 28, 33);
const ELEVATED: Color32 = Color32::from_rgb(36, 39, 46);
const MUTED: Color32 = Color32::from_rgb(153, 157, 169);
const WHITE: Color32 = Color32::from_rgb(235, 237, 241);
const RED: Color32 = Color32::from_rgb(255, 70, 85);
const GREEN: Color32 = Color32::from_rgb(97, 205, 154);

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = PANEL;
    visuals.faint_bg_color = ELEVATED;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.inactive.bg_fill = ELEVATED;
    visuals.selection.bg_fill = Color32::from_rgb(58, 79, 96);
    ctx.set_visuals(visuals);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(9.0, 9.0);
    style.spacing.button_padding = egui::vec2(11.0, 6.0);
    ctx.set_style(style);
}

fn heading(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(RichText::new(title).color(WHITE).size(25.0).strong());
    ui.label(RichText::new(sub).color(MUTED).size(12.0));
    ui.add_space(12.0);
}

fn metric(ui: &mut egui::Ui, label: &str, value: &str, note: &str) {
    egui::Frame::new().fill(PANEL).corner_radius(8.0).inner_margin(16.0).show(ui, |ui| {
        ui.set_min_width(146.0);
        ui.label(RichText::new(label.to_uppercase()).color(MUTED).size(11.0));
        ui.label(RichText::new(value).color(WHITE).size(25.0).strong());
        ui.label(RichText::new(note).color(MUTED).size(10.0));
    });
}

fn breakdown_panel(ui: &mut egui::Ui, matches: &[&Match], by_agent: bool) {
    let mut groups = std::collections::BTreeMap::<String, (usize, usize, usize)>::new();
    for m in matches {
        let name = if by_agent { &m.agent } else { &m.map };
        let entry = groups.entry(name.clone()).or_default();
        entry.0 += 1;
        if m.won.is_some() { entry.1 += 1; }
        if m.won == Some(true) { entry.2 += 1; }
    }
    let mut ordered: Vec<_> = groups.into_iter().collect();
    ordered.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
    egui::Frame::new().fill(PANEL).corner_radius(9.0)
        .inner_margin(egui::Margin::same(17)).show(ui, |ui| {
            ui.set_min_width(300.0);
            ui.label(RichText::new(if by_agent { "AGENT PERFORMANCE" } else { "MAP PERFORMANCE" }).color(WHITE).strong().size(14.0));
            ui.label(RichText::new("Sorted by matches played").color(MUTED).size(11.0));
            ui.add_space(12.0);
            for (label, (played, decided, wins)) in ordered {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&label).color(WHITE).size(13.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(if decided > 0 {
                            format!("{:.0}% WR", 100.0 * wins as f64 / decided as f64)
                        } else { "N/A".into() }).color(if decided > 0 { GREEN } else { MUTED }).size(12.0));
                    });
                });
                let ratio = if decided > 0 { wins as f32 / decided as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(ratio).desired_width(ui.available_width()).fill(GREEN));
                ui.label(RichText::new(format!("{played} played · {wins} wins · {decided} decided")).color(MUTED).size(10.0));
                ui.add_space(10.0);
            }
        });
}

fn is_round_mode(mode: &str) -> bool {
    !mode.to_ascii_lowercase().contains("deathmatch")
}

fn outcome_label(won: Option<bool>) -> (&'static str, Color32) {
    match won { Some(true) => ("VICTORY", GREEN), Some(false) => ("DEFEAT", RED), None => ("RESULT N/A", MUTED) }
}

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const SERVICE: &str = "recon-henrikdev";
const SECRET_USER: &str = "personal-api-key";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Settings {
    name: String,
    tag: String,
    region: String,
    #[serde(default = "default_sync_count")]
    sync_count: usize,
}

fn default_sync_count() -> usize { 10 }

#[derive(Clone, Debug)]
struct Match {
    id: String,
    map: String,
    agent: String,
    mode: String,
    played_at: i64,
    kills: i64,
    deaths: i64,
    assists: i64,
    score: i64,
    rounds: i64,
    won: Option<bool>,
    headshots: i64,
    bodyshots: i64,
    legshots: i64,
}

impl Match {
    fn acs(&self) -> f64 {
        if self.rounds > 0 {
            self.score as f64 / self.rounds as f64
        } else {
            0.0
        }
    }

    fn hs_pct(&self) -> Option<f64> {
        let total = self.headshots + self.bodyshots + self.legshots;
        (total > 0).then_some(100.0 * self.headshots as f64 / total as f64)
    }
}

fn app_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("recon")
}

fn config_path() -> PathBuf {
    app_dir().join("settings.json")
}

fn load_settings() -> Settings {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| Settings {
            region: "ap".into(),
            sync_count: default_sync_count(),
            ..Settings::default()
        })
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(app_dir()).map_err(|e| e.to_string())?;
    let data = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), data).map_err(|e| e.to_string())
}

fn db() -> Result<Connection, String> {
    std::fs::create_dir_all(app_dir()).map_err(|e| e.to_string())?;
    let connection = Connection::open(app_dir().join("matches.db")).map_err(|e| e.to_string())?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS matches (
                id TEXT PRIMARY KEY,
                map TEXT NOT NULL,
                agent TEXT NOT NULL,
                mode TEXT NOT NULL,
                played_at INTEGER NOT NULL,
                kills INTEGER NOT NULL,
                deaths INTEGER NOT NULL,
                assists INTEGER NOT NULL,
                score INTEGER NOT NULL,
                rounds INTEGER NOT NULL,
                won INTEGER,
                headshots INTEGER NOT NULL,
                bodyshots INTEGER NOT NULL,
                legshots INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS match_details (
                 match_id TEXT PRIMARY KEY,
                 payload_json TEXT NOT NULL
             );",
        )
        .map_err(|e| e.to_string())?;
    Ok(connection)
}

fn load_matches() -> Result<Vec<Match>, String> {
    let connection = db()?;
    let mut statement = connection
        .prepare("SELECT id,map,agent,mode,played_at,kills,deaths,assists,score,rounds,won,headshots,bodyshots,legshots FROM matches ORDER BY played_at DESC LIMIT 200")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |r| {
            Ok(Match {
                id: r.get(0)?,
                map: r.get(1)?,
                agent: r.get(2)?,
                mode: r.get(3)?,
                played_at: r.get(4)?,
                kills: r.get(5)?,
                deaths: r.get(6)?,
                assists: r.get(7)?,
                score: r.get(8)?,
                rounds: r.get(9)?,
                won: r.get::<_, Option<i64>>(10)?.map(|x| x != 0),
                headshots: r.get(11)?,
                bodyshots: r.get(12)?,
                legshots: r.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn persist_matches(matches: &[Match]) -> Result<(), String> {
    let mut connection = db()?;
    let tx = connection.transaction().map_err(|e| e.to_string())?;
    for m in matches {
        tx.execute(
            "INSERT OR REPLACE INTO matches
            (id,map,agent,mode,played_at,kills,deaths,assists,score,rounds,won,headshots,bodyshots,legshots)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                m.id, m.map, m.agent, m.mode, m.played_at, m.kills,
                m.deaths, m.assists, m.score, m.rounds,
                m.won.map(i64::from), m.headshots, m.bodyshots, m.legshots
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

// Keep the original, richer HenrikDev v3 payload for round-by-round
// detail screens. No additional API call is needed to view cached games.
fn persist_match_details(payload: &Value) -> Result<(), String> {
    let Some(games) = payload.get("data").and_then(Value::as_array) else { return Ok(()); };
    let mut connection = db()?;
    let tx = connection.transaction().map_err(|e| e.to_string())?;
    for game in games {
        let Some(id) = game.pointer("/metadata/matchid").and_then(Value::as_str) else { continue; };
        let raw = serde_json::to_string(game).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT OR REPLACE INTO match_details(match_id,payload_json) VALUES (?1,?2)",
            params![id, raw],
        ).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn load_match_detail(id: &str) -> Option<Value> {
    let connection = db().ok()?;
    let raw: String = connection.query_row(
        "SELECT payload_json FROM match_details WHERE match_id=?1",
        params![id], |row| row.get(0),
    ).ok()?;
    serde_json::from_str(&raw).ok()
}

fn score_for(game: &Value, team: &str) -> Option<i64> {
    game.pointer(&format!("/teams/{}/rounds_won", team.to_ascii_lowercase()))
        .and_then(Value::as_i64)
        .or_else(|| game.pointer(&format!("/teams/{}/rounds_won", team)).and_then(Value::as_i64))
}

fn player_side(game: &Value, name: &str, tag: &str) -> Option<String> {
    game.pointer("/players/all_players")?.as_array()?.iter().find(|p| {
        p.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(name))
            && p.get("tag").and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case(tag))
    })?.get("team")?.as_str().map(|s| s.to_ascii_lowercase())
}

fn round_winners(game: &Value, team: &str) -> Vec<Option<bool>> {
    let Some(rounds) = game.get("rounds").and_then(Value::as_array) else { return vec![] };
    rounds.iter().map(|round| {
        round.get("winning_team").and_then(Value::as_str).and_then(|winner| {
            if winner.eq_ignore_ascii_case("red") || winner.eq_ignore_ascii_case("blue") {
                Some(winner.eq_ignore_ascii_case(team))
            } else { None }
        })
    }).collect()
}

fn get_str<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}

fn get_int(value: &Value, pointer: &str) -> i64 {
    value.pointer(pointer).and_then(Value::as_i64).unwrap_or(0)
}

fn parse_matches(payload: &Value, settings: &Settings) -> Vec<Match> {
    let Some(games) = payload.get("data").and_then(Value::as_array) else {
        return vec![];
    };
    games
        .iter()
        .filter_map(|game| {
            let id = get_str(game, "/metadata/matchid");
            if id.is_empty() {
                return None;
            }
            let players = game.pointer("/players/all_players")?.as_array()?;
            let p = players.iter().find(|p| {
                get_str(p, "/name").eq_ignore_ascii_case(&settings.name)
                    && get_str(p, "/tag").eq_ignore_ascii_case(&settings.tag)
            })?;
            let side = get_str(p, "/team").to_ascii_lowercase();
            let us = get_int(game, &format!("/teams/{side}/rounds_won"));
            let other = if side == "red" { "blue" } else { "red" };
            let them = get_int(game, &format!("/teams/{other}/rounds_won"));
            let rounds = get_int(game, "/metadata/rounds_played").max(us + them);
            let won = if side == "red" || side == "blue" {
                Some(us > them)
            } else {
                None
            };
            Some(Match {
                id: id.to_string(),
                map: get_str(game, "/metadata/map").to_string(),
                agent: get_str(p, "/character").to_string(),
                mode: get_str(game, "/metadata/mode").to_string(),
                played_at: get_int(game, "/metadata/game_start"),
                kills: get_int(p, "/stats/kills"),
                deaths: get_int(p, "/stats/deaths"),
                assists: get_int(p, "/stats/assists"),
                score: get_int(p, "/stats/score"),
                rounds,
                won,
                headshots: get_int(p, "/stats/headshots"),
                bodyshots: get_int(p, "/stats/bodyshots"),
                legshots: get_int(p, "/stats/legshots"),
            })
        })
        .collect()
}

// HenrikDev's v3 endpoint is intentionally limited to ten full matches.
// Older history comes from the paginated, compact lifetime endpoint. It only
// contains games already indexed by HenrikDev, so requesting N does not
// guarantee that N exist.
fn parse_lifetime_matches(payload: &Value) -> Vec<Match> {
    let Some(items) = payload.get("data").and_then(Value::as_array) else {
        return vec![];
    };
    items.iter().filter_map(|entry| {
        let meta = entry.get("meta")?;
        let stats = entry.get("stats")?;
        let id = meta.get("id")?.as_str()?.trim();
        let started_at = meta.get("started_at")?.as_str()?;
        let played_at = chrono::DateTime::parse_from_rfc3339(started_at).ok()?.timestamp();
        let map = meta.pointer("/map/name")?.as_str()?;
        let agent = stats.pointer("/character/name")?.as_str()?;
        let kills = stats.get("kills")?.as_i64()?;
        let deaths = stats.get("deaths")?.as_i64()?;
        let assists = stats.get("assists")?.as_i64()?;
        let mode = meta.get("mode").and_then(Value::as_str).unwrap_or("Unknown");
        let score = stats.get("score").and_then(Value::as_i64).unwrap_or(0);
        let shots = stats.get("shots");
        let side = stats.get("team").and_then(Value::as_str).unwrap_or("");
        let team = entry.get("teams");
        let red = team.and_then(|v| v.get("red")).and_then(Value::as_i64);
        let blue = team.and_then(|v| v.get("blue")).and_then(Value::as_i64);
        let rounds = red.zip(blue).map(|(r,b)| r+b).unwrap_or(0);
        let won = match (side.to_ascii_lowercase().as_str(), red, blue) {
            ("red", Some(r), Some(b)) if r != b => Some(r > b),
            ("blue", Some(r), Some(b)) if r != b => Some(b > r),
            _ => None,
        };
        if id.is_empty() || played_at <= 0 { return None; }
        Some(Match {
            id: id.into(), map: map.into(), agent: agent.into(),
            mode: mode.into(), played_at, kills, deaths, assists, score,
            rounds, won,
            headshots: shots.and_then(|v| v.get("head")).and_then(Value::as_i64).unwrap_or(0),
            bodyshots: shots.and_then(|v| v.get("body")).and_then(Value::as_i64).unwrap_or(0),
            legshots: shots.and_then(|v| v.get("leg")).and_then(Value::as_i64).unwrap_or(0),
        })
    }).collect()
}

fn fetch_matches(settings: &Settings, key: &str) -> Result<Vec<Match>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| e.to_string())?;
    let region = urlencoding::encode(settings.region.trim());
    let name = urlencoding::encode(settings.name.trim());
    let tag = urlencoding::encode(settings.tag.trim());
    let url = format!("https://api.henrikdev.xyz/valorant/v3/matches/{region}/{name}/{tag}");
    // This is the provider's maximum for full match details.
    let response = client.get(url).header("Authorization", key)
        .query(&[("size", "10")]).send()
        .map_err(|e| format!("Recent match fetch failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HenrikDev recent matches returned HTTP {status}. Check credentials, region and rate limits."));
    }
    let payload: Value = response.json().map_err(|e| format!("Invalid recent-match response: {e}"))?;
    if !payload.get("data").is_some_and(Value::is_array) {
        return Err("HenrikDev returned no recent match list; API response may have changed.".into());
    }
    let target = settings.sync_count.clamp(5, 50);
    persist_match_details(&payload)?;
    let mut matches = parse_matches(&payload, settings);
    let mut ids: std::collections::HashSet<String> = matches.iter().map(|m| m.id.clone()).collect();

    if target > matches.len() {
        // Lifetime data is indexed by the provider, and may be incomplete. We
        // fetch sequentially to stay well under a Basic key's rate limit.
        let history_url = format!(
            "https://api.henrikdev.xyz/valorant/v1/lifetime/matches/{region}/{name}/{tag}"
        );
        for page in 1..=5 {
            let response = match client.get(&history_url).header("Authorization", key)
                .query(&[("page", page.to_string()), ("size", "50".to_owned())]).send() {
                    Ok(response) if response.status().is_success() => response,
                    _ => break, // Keep the successfully retrieved recent matches.
                };
            let Ok(payload) = response.json::<Value>() else { break };
            let history = parse_lifetime_matches(&payload);
            if history.is_empty() { break; }
            let page_len = history.len();
            for m in history {
                if ids.insert(m.id.clone()) { matches.push(m); }
            }
            if matches.len() >= target || page_len < 50 { break; }
        }
    }
    matches.sort_by(|a,b| b.played_at.cmp(&a.played_at));
    matches.truncate(target);
    Ok(matches)
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Page {
    Overview,
    Matches,
    Analytics,
    Settings,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum OverviewChart { Acs, Kd }

struct Recon {
    page: Page,
    settings: Settings,
    key_input: String,
    matches: Vec<Match>,
    selected: Option<String>,
    mode_filter: String,
    search: String,
    sidebar_collapsed: bool,
    overview_mode: String,
    overview_chart: OverviewChart,
    worker: Option<Receiver<Result<usize, String>>>,
    status: String,
}

impl Recon {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        let (matches, status) = match load_matches() {
            Ok(matches) => (matches, "Ready. Saved matches work offline.".into()),
            Err(err) => (vec![], format!("Local database error: {err}")),
        };
        Self {
            page: Page::Overview,
            settings: load_settings(),
            key_input: String::new(),
            matches,
            selected: None,
            mode_filter: "All".into(),
            search: String::new(),
            sidebar_collapsed: false,
            overview_mode: "All".into(),
            overview_chart: OverviewChart::Acs,
            worker: None,
            status,
        }
    }

    fn save(&mut self) {
        match save_settings(&self.settings) {
            Ok(()) => self.status = "Settings saved.".into(),
            Err(e) => {
                self.status = format!("Unable to save settings: {e}");
                return;
            }
        }
        if !self.key_input.is_empty() {
            match keyring::Entry::new(SERVICE, SECRET_USER)
                .map_err(|e| e.to_string())
                .and_then(|entry| entry.set_password(&self.key_input).map_err(|e| e.to_string()))
            {
                Ok(()) => {
                    self.status = "Settings and API key saved securely.".into();
                    self.key_input.clear();
                }
                Err(e) => self.status = format!("Could not save API key to credential store: {e}"),
            }
        }
    }

    fn sync(&mut self) {
        if self.worker.is_some() {
            return;
        }
        if self.settings.name.trim().is_empty() || self.settings.tag.trim().is_empty() {
            self.status = "Enter Riot ID name and tag in Settings first.".into();
            self.page = Page::Settings;
            return;
        }
        let settings = self.settings.clone();
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        self.status = "Fetching recent matches…".into();
        std::thread::spawn(move || {
            let result = (|| {
                let key = keyring::Entry::new(SERVICE, SECRET_USER)
                    .map_err(|e| e.to_string())?
                    .get_password()
                    .map_err(|_| "No API key saved. Add one in Settings.".to_string())?;
                let matches = fetch_matches(&settings, &key)?;
                let count = matches.len();
                persist_matches(&matches)?;
                Ok(count)
            })();
            let _ = tx.send(result);
        });
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Overview", "Your performance at a glance · cached matches");
        if self.matches.is_empty() {
            ui.label("No matches saved yet. Add your Riot ID and API key in Settings, then sync.");
            if ui.button("Open Settings").clicked() { self.page = Page::Settings; }
            return;
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new("GAME MODE").color(MUTED).size(11.0));
            for mode in ["All", "Competitive", "Unrated"] {
                let chosen = self.overview_mode == mode;
                if ui.add(egui::Button::new(RichText::new(mode).color(if chosen { WHITE } else { MUTED }))
                    .fill(if chosen { ELEVATED } else { PANEL })).clicked() {
                    self.overview_mode = mode.to_owned();
                }
            }
        });
        ui.add_space(8.0);
        let games: Vec<&Match> = self.matches.iter()
            .filter(|m| is_round_mode(&m.mode)
                && (self.overview_mode == "All" || m.mode.eq_ignore_ascii_case(&self.overview_mode)))
            .collect();
        if games.is_empty() {
            ui.label("No matches in this mode yet.");
            return;
        }

        let wins = games.iter().filter(|m| m.won == Some(true)).count();
        let decided = games.iter().filter(|m| m.won.is_some()).count();
        let kills: i64 = games.iter().map(|m| m.kills).sum();
        let deaths: i64 = games.iter().map(|m| m.deaths).sum();
        let scored: i64 = games.iter().filter(|m| m.score > 0 && m.rounds > 0).map(|m| m.score).sum();
        let rounds: i64 = games.iter().filter(|m| m.score > 0 && m.rounds > 0).map(|m| m.rounds).sum();
        ui.horizontal_wrapped(|ui| {
            metric(ui, "Matches", &games.len().to_string(), "Cached · filtered");
            metric(ui, "Win rate",
                &if decided > 0 {format!("{:.0}%", 100.0 * wins as f64 / decided as f64)} else {"N/A".into()},
                &format!("{wins} wins / {decided} decided"));
            metric(ui, "K/D ratio", &if deaths > 0 {format!("{:.2}", kills as f64 / deaths as f64)} else {"N/A".into()},
                &format!("{kills} kills / {deaths} deaths"));
            metric(ui, "Average ACS", &if rounds > 0 {format!("{:.0}", scored as f64 / rounds as f64)} else {"N/A".into()},
                "Round-weighted · available scores");
        });

        ui.add_space(16.0);
        egui::Frame::new().fill(PANEL).corner_radius(9.0)
            .inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("RECENT RESULTS").color(WHITE).strong());
                    ui.label(RichText::new("Oldest to newest · last 20 filtered matches").color(MUTED).size(11.0));
                });
                ui.add_space(5.0);
                let results: Vec<&Match> = games.iter().take(20).copied().rev().collect();
                ui.horizontal_wrapped(|ui| {
                    for (index, m) in results.iter().enumerate() {
                        let (label, color) = outcome_label(m.won);
                        let short = if m.won == Some(true) { "W" } else if m.won == Some(false) { "L" } else { "?" };
                        let response = ui.add_sized([31.0, 29.0],
                            egui::Button::new(RichText::new(short).color(color).strong())
                                .fill(ELEVATED));
                        if response.clicked() {
                            self.selected = Some(m.id.clone());
                            self.page = Page::Matches;
                        }
                        response.on_hover_text(format!("{} · {} · {} · {}", index + 1, m.map, m.agent, label));
                    }
                });
            });

        ui.add_space(16.0);
        egui::Frame::new().fill(PANEL).corner_radius(9.0)
            .inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                ui.label(RichText::new("PERFORMANCE TREND").color(WHITE).strong());
                ui.label(RichText::new("Chronological · valid round-based matches only").color(MUTED).size(11.0));
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.overview_chart, OverviewChart::Acs, "ACS");
                    ui.selectable_value(&mut self.overview_chart, OverviewChart::Kd, "K/D");
                });
                let points: Vec<[f64; 2]> = games.iter().rev().filter_map(|m| {
                    match self.overview_chart {
                        OverviewChart::Acs if m.score > 0 && m.rounds > 0 => Some(m.acs()),
                        OverviewChart::Kd if m.deaths > 0 => Some(m.kills as f64 / m.deaths as f64),
                        _ => None,
                    }
                }).enumerate().map(|(i, v)| [i as f64 + 1.0, v]).collect();
                if points.is_empty() {
                    ui.label("No usable data for the selected trend.");
                } else {
                    egui_plot::Plot::new("overview_trend")
                        .height(230.0)
                        .allow_drag(false)
                        .allow_zoom(false)
                        .show(ui, |plot| {
                            plot.line(egui_plot::Line::new(egui_plot::PlotPoints::from(points))
                                .name(match self.overview_chart { OverviewChart::Acs => "ACS", OverviewChart::Kd => "K/D" })
                                .color(RED).width(2.5_f32));
                        });
                }
            });

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("MOST PLAYED AGENTS").color(WHITE).strong());
            if ui.button("Explore analytics").clicked() { self.page = Page::Analytics; }
        });
        let mut agents = std::collections::BTreeMap::<String, (usize, usize, usize)>::new();
        for m in &games {
            let item = agents.entry(m.agent.clone()).or_default();
            item.0 += 1;
            if m.won.is_some() { item.1 += 1; }
            if m.won == Some(true) { item.2 += 1; }
        }
        let mut sorted: Vec<_> = agents.into_iter().collect();
        sorted.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        egui::Frame::new().fill(PANEL).corner_radius(8.0)
            .inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                for (agent, (played, decided, won)) in sorted.into_iter().take(3) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&agent).color(WHITE).strong());
                        ui.label(RichText::new(format!("{played} played")).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(if decided > 0 {
                                format!("{:.0}% win rate", 100.0 * won as f64 / decided as f64)
                            } else { "N/A".into() }).color(GREEN));
                        });
                    });
                }
            });

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("RECENT MATCHES").color(WHITE).strong());
            if ui.button("View all").clicked() { self.page = Page::Matches; }
        });
        let recents: Vec<Match> = games.iter().take(5).map(|m| (**m).clone()).collect();
        for m in &recents { self.match_card(ui, m); }
        ui.label(RichText::new("Statistics reflect locally cached matches only, not your entire VALORANT career.").color(MUTED).size(10.0));
    }

    fn match_card(&mut self, ui: &mut egui::Ui, m: &Match) {
        let (result, accent) = outcome_label(m.won);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 72.0),
            egui::Sense::click(),
        );
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let fill = if response.hovered() { ELEVATED } else { PANEL };
        ui.painter().rect_filled(rect, 7.0, fill);
        // Draw our own result indicator rather than relying on missing glyphs.
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(rect.left() + 12.0, rect.top() + 16.0),
                egui::vec2(3.0, 40.0),
            ),
            1.5,
            accent,
        );
        ui.allocate_ui_at_rect(rect.shrink2(egui::vec2(24.0, 10.0)), |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(115.0);
                    ui.label(RichText::new(&m.map).size(15.0).color(WHITE).strong());
                    ui.label(RichText::new(format!("{} · {}", m.agent, m.mode)).size(11.0).color(MUTED));
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{}/{}/{}", m.kills, m.deaths, m.assists)).size(15.0).color(WHITE));
                    ui.label(RichText::new("K / D / A").size(10.0).color(MUTED));
                });
                if is_round_mode(&m.mode) && m.rounds > 0 {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("{:.0}", m.acs())).size(15.0).color(WHITE));
                        ui.label(RichText::new("ACS").size(10.0).color(MUTED));
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("View").color(MUTED).size(11.0));
                    ui.label(RichText::new(result).color(accent).strong().size(11.0));
                });
            });
        });
        if response.clicked() {
            self.selected = Some(m.id.clone());
            self.page = Page::Matches;
        }
        ui.add_space(6.0);
    }

    fn match_list(&mut self, ui: &mut egui::Ui) {
        if let Some(m) = self.matches.iter().find(|m| Some(m.id.as_str()) == self.selected.as_deref()).cloned() {
            if ui.button("Back to matches").clicked() {
                self.selected = None;
                return;
            }
            ui.add_space(10.0);
            heading(ui, &format!("{}  /  {}", m.map, m.agent), &format!("{} · Personal match breakdown", m.mode));
            let (outcome, color) = outcome_label(m.won);
            let detail = load_match_detail(&m.id);
            let timestamp = chrono::DateTime::from_timestamp(m.played_at, 0)
                .map(|dt| dt.format("%d %b %Y · %H:%M UTC").to_string())
                .unwrap_or_else(|| "Date unavailable".into());
            egui::Frame::new().fill(PANEL).corner_radius(8.0)
                .inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(outcome).color(color).strong().size(18.0));
                        ui.separator();
                        ui.label(RichText::new(timestamp).color(MUTED).size(12.0));
                        if let Some(game) = &detail {
                            if let Some(side) = player_side(game, &self.settings.name, &self.settings.tag) {
                                let opponent = if side == "red" { "blue" } else { "red" };
                                if let (Some(ours), Some(theirs)) = (score_for(game, &side), score_for(game, opponent)) {
                                    ui.separator();
                                    ui.label(RichText::new(format!("{ours} : {theirs}")).color(WHITE).size(20.0).strong());
                                }
                            }
                        }
                    });
                });
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                metric(ui, "Kills", &m.kills.to_string(), "Eliminations");
                metric(ui, "Deaths", &m.deaths.to_string(), "This match");
                metric(ui, "Assists", &m.assists.to_string(), "This match");
                metric(ui, "K/D", &format!("{:.2}", m.kills as f64 / m.deaths.max(1) as f64), "Kills per death");
            });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if is_round_mode(&m.mode) && m.rounds > 0 && m.score > 0 {
                    metric(ui, "ACS", &format!("{:.0}", m.acs()), "Score per round");
                    metric(ui, "Rounds", &m.rounds.to_string(), "Match duration");
                    metric(ui, "Kills / round", &format!("{:.2}", m.kills as f64 / m.rounds as f64), "Measured average");
                }
                let hs = m.hs_pct().map(|v| format!("{v:.1}%")).unwrap_or_else(|| "N/A".into());
                metric(ui, "HS hit share", &hs, "Of recorded hits");
            });
            ui.add_space(14.0);
            egui::Frame::new().fill(PANEL).corner_radius(8.0)
                .inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                    ui.label(RichText::new("HIT DISTRIBUTION").color(WHITE).strong());
                    let total = m.headshots + m.bodyshots + m.legshots;
                    if total > 0 {
                        let available_width = (ui.available_width() - 4.0).max(80.0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(available_width, 13.0), egui::Sense::hover());
                        let mut left = rect.left();
                        for (count, col) in [(m.headshots, RED), (m.bodyshots, GREEN), (m.legshots, MUTED)] {
                            let width = available_width * (count as f32 / total as f32);
                            if width > 0.0 {
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(width, rect.height())),
                                    2.0, col,
                                );
                            }
                            left += width;
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("Head: {} ({:.1}%)", m.headshots, 100.0 * m.headshots as f64 / total as f64));
                            ui.label(format!("Body: {} ({:.1}%)", m.bodyshots, 100.0 * m.bodyshots as f64 / total as f64));
                            ui.label(format!("Legs: {} ({:.1}%)", m.legshots, 100.0 * m.legshots as f64 / total as f64));
                        });
                    } else {
                        ui.label(RichText::new("No hit-location information available for this match.").color(MUTED));
                    }
                    ui.label(RichText::new("Hit share is not the percentage of kills finished with headshots.").color(MUTED).size(10.0));
                });
            ui.add_space(14.0);
            egui::Frame::new().fill(PANEL).corner_radius(8.0)
                .inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                    ui.label(RichText::new("ROUND TIMELINE").color(WHITE).strong());
                    match detail.as_ref().and_then(|game| {
                        player_side(game, &self.settings.name, &self.settings.tag)
                            .map(|side| round_winners(game, &side))
                    }) {
                        Some(winners) if !winners.is_empty() => {
                            ui.label(RichText::new("Your team won (green) or lost (red) each round.").color(MUTED).size(11.0));
                            ui.horizontal_wrapped(|ui| {
                                for (index, winner) in winners.iter().enumerate() {
                                    let (label, color) = match winner {
                                        Some(true) => ("W", GREEN),
                                        Some(false) => ("L", RED),
                                        None => ("?", MUTED),
                                    };
                                    ui.label(RichText::new(format!("{} {}", index + 1, label)).color(color).strong().size(12.0));
                                }
                            });
                            let victories = winners.iter().filter(|w| **w == Some(true)).count();
                            let defeats = winners.iter().filter(|w| **w == Some(false)).count();
                            ui.label(RichText::new(format!("{} rounds won · {} rounds lost", victories, defeats)).color(MUTED));
                        }
                        _ => { ui.label(RichText::new("Round data is unavailable for this cached match. New recent-match syncs may provide it; lifetime history is more limited.").color(MUTED)); },
                    };
                });
            ui.add_space(14.0);
            ui.label(RichText::new("This report describes recorded outcomes, not positioning or crosshair mistakes. Video analysis will be a separate feature.").color(MUTED).size(11.0));
            ui.label(RichText::new(format!("Match ID: {}", m.id)).color(MUTED).size(10.0));
            return;
        }
        heading(ui, "Match history", "Browse cached results and inspect individual matches");
        ui.horizontal_wrapped(|ui| {
            ui.label("Mode");
            egui::ComboBox::from_id_salt("mode_filter").selected_text(&self.mode_filter).show_ui(ui, |ui| {
                for mode in ["All", "Competitive", "Unrated", "Swiftplay", "Team Deathmatch", "Deathmatch", "Other"] {
                    ui.selectable_value(&mut self.mode_filter, mode.to_owned(), mode);
                }
            });
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Map or agent").desired_width(175.0));
            ui.label(RichText::new(format!("{} cached", self.matches.len())).color(MUTED));
        });
        ui.add_space(10.0);
        let visible: Vec<Match> = self.matches.iter().filter(|m| {
            (self.mode_filter == "All" || if self.mode_filter == "Other" {
                !["competitive", "unrated", "swiftplay", "team deathmatch", "deathmatch"]
                    .contains(&m.mode.to_lowercase().as_str())
            } else { m.mode.eq_ignore_ascii_case(&self.mode_filter) })
                && (self.search.trim().is_empty() || m.map.to_lowercase().contains(&self.search.trim().to_lowercase())
                    || m.agent.to_lowercase().contains(&self.search.trim().to_lowercase()))
        }).cloned().collect();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if visible.is_empty() { ui.label("No matches match these filters."); }
            for m in &visible { self.match_card(ui, m); }
        });

    }

    fn analytics_ui(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Analytics", "Agent and map performance across cached round-based matches");
        let supported: Vec<&Match> = self.matches.iter().filter(|m| is_round_mode(&m.mode)).collect();
        if supported.is_empty() {
            ui.label("No supported matches to analyze yet.");
            return;
        }
        let decided = supported.iter().filter(|m| m.won.is_some()).count();
        let won = supported.iter().filter(|m| m.won == Some(true)).count();
        let agents = supported.iter().map(|m| m.agent.as_str()).collect::<std::collections::HashSet<_>>().len();
        let maps = supported.iter().map(|m| m.map.as_str()).collect::<std::collections::HashSet<_>>().len();
        ui.horizontal_wrapped(|ui| {
            metric(ui, "Matches", &supported.len().to_string(), "Round-based modes");
            metric(ui, "Win rate", &if decided > 0 { format!("{:.0}%", 100.0 * won as f64 / decided as f64) } else { "N/A".to_owned() }, "Known outcomes");
            metric(ui, "Agents played", &agents.to_string(), "Unique agents");
            metric(ui, "Maps played", &maps.to_string(), "Unique maps");
        });
        ui.add_space(18.0);
        // Avoid the enormous empty right-hand side of the old tiny tables.
        if ui.available_width() >= 850.0 {
            ui.columns(2, |cols| {
                breakdown_panel(&mut cols[0], &supported, true);
                breakdown_panel(&mut cols[1], &supported, false);
            });
        } else {
            breakdown_panel(ui, &supported, true);
            ui.add_space(14.0);
            breakdown_panel(ui, &supported, false);
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Only matches with known results contribute to win-rate percentages. Single-game 100% win rates are not meaningful evidence of a strong agent or map.").color(MUTED).size(11.0));
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Settings", "Account, API access and local data");
        egui::Frame::new().fill(PANEL).corner_radius(9.0).inner_margin(egui::Margin::same(18)).show(ui, |ui| {
            ui.set_max_width(650.0);
            ui.label(RichText::new("RIOT ACCOUNT").color(WHITE).strong());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Player name").color(MUTED).size(11.0));
                    ui.add(egui::TextEdit::singleline(&mut self.settings.name).desired_width(225.0));
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new("Tag").color(MUTED).size(11.0));
                    ui.add(egui::TextEdit::singleline(&mut self.settings.tag).desired_width(100.0));
                });
            });
            ui.label(RichText::new("Region").color(MUTED).size(11.0));
            egui::ComboBox::from_id_salt("region").selected_text(&self.settings.region).show_ui(ui, |ui| {
                for region in ["ap", "eu", "na", "kr", "latam", "br"] {
                    ui.selectable_value(&mut self.settings.region, region.into(), region);
                }
            });
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(RichText::new("MATCH SYNC").color(WHITE).strong());
            ui.horizontal(|ui| {
                ui.label("Request last");
                egui::ComboBox::from_id_salt("sync_count")
                    .selected_text(format!("{} matches", self.settings.sync_count))
                    .show_ui(ui, |ui| {
                        for count in [5_usize, 10, 20, 50] {
                            ui.selectable_value(&mut self.settings.sync_count, count, format!("{count} matches"));
                        }
                    });
            });
            ui.label(RichText::new("Recent detail is limited to 10; larger syncs use cached lifetime history and may have gaps.").color(MUTED).size(11.0));
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(RichText::new("HENRIKDEV API KEY").color(WHITE).strong());
            ui.label(RichText::new("Saved to your OS credential store. Leaving the field blank preserves the existing key.").color(MUTED).size(11.0));
            ui.add(egui::TextEdit::singleline(&mut self.key_input).password(true).hint_text("Enter new API key").desired_width(360.0));
            if ui.button(RichText::new("Save settings").color(WHITE)).clicked() { self.save(); }
        });
        ui.add_space(16.0);
        egui::Frame::new().fill(PANEL).corner_radius(9.0).inner_margin(egui::Margin::same(18)).show(ui, |ui| {
            ui.label(RichText::new("LOCAL STORAGE").color(WHITE).strong());
            ui.label(RichText::new(app_dir().display().to_string()).color(MUTED).size(12.0));
            ui.label(RichText::new("Your match database stays on this PC. Nothing is automatically uploaded to RECON cloud services.").color(MUTED).size(11.0));
        });
    }
}

impl eframe::App for Recon {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.worker {
            match rx.try_recv() {
                Ok(result) => {
                    self.status = match result {
                        Ok(count) => {
                            match load_matches() {
                                Ok(matches) => self.matches = matches,
                                Err(e) => return self.status = format!("Reload failed: {e}"),
                            }
                            if count < self.settings.sync_count { format!("Synced {count} match(es); requested {}. Provider may limit available history.", self.settings.sync_count) } else { format!("Synced {count} recent matches.") }
                        }
                        Err(e) => e,
                    };
                    self.worker = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = "Sync worker disconnected unexpectedly.".into();
                    self.worker = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(150));
                }
            }
        }
        egui::SidePanel::left("recon_sidebar")
            .resizable(false)
            .exact_width(if self.sidebar_collapsed { 58.0 } else { 182.0 })
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::same(12)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(if self.sidebar_collapsed { "R" } else { "RECON" }).color(RED).strong().size(21.0));
                    if ui.small_button(if self.sidebar_collapsed { "›" } else { "‹" }).clicked() {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                    }
                });
                ui.separator();
                for (p, label, compact) in [
                    (Page::Overview, "Overview", "O"),
                    (Page::Matches, "Matches", "M"),
                    (Page::Analytics, "Analytics", "A"),
                    (Page::Settings, "Settings", "S"),
                ] {
                    let selected = self.page == p;
                    if ui.add_sized([ui.available_width(), 33.0], egui::Button::new(
                        RichText::new(if self.sidebar_collapsed { compact } else { label })
                            .color(if selected { WHITE } else { MUTED })
                    ).fill(if selected { ELEVATED } else { PANEL })).clicked() {
                        self.page = p;
                    }
                }
                ui.add_space(12.0);
                ui.separator();
                if !self.sidebar_collapsed {
                    ui.label(RichText::new("LOCAL FIRST").color(MUTED).size(10.0));
                    ui.label(RichText::new(format!("{} matches cached", self.matches.len())).color(WHITE).size(11.0));
                }
            });
        egui::TopBottomPanel::top("recon_toolbar")
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(16, 10)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}#{}", self.settings.name, self.settings.tag)).color(WHITE).strong());
                    ui.label(RichText::new(format!(" · {}", self.settings.region.to_uppercase())).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(self.worker.is_none(), egui::Button::new("Sync matches")).clicked() {
                            self.sync();
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("status").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(14, 6))).show(ctx, |ui| {
            ui.label(RichText::new(&self.status).color(MUTED).size(11.0));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(18))).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.page {
                    Page::Overview => self.overview(ui),
                    Page::Matches => self.match_list(ui),
                    Page::Analytics => self.analytics_ui(ui),
                    Page::Settings => self.settings_ui(ui),
                }
            });
        });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "RECON · VALORANT Match Tracker",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 740.0]).with_min_inner_size([700.0, 480.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(Recon::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_empty_api_data() {
        let settings = Settings { name: "Me".into(), tag: "TAG".into(), region: "ap".into(), sync_count: 10 };
        assert!(parse_matches(&serde_json::json!({"data": []}), &settings).is_empty());
    }
    #[test]
    fn acs_and_headshot_share() {
        let m = Match {
            id: "x".into(), map: "Ascent".into(), agent: "Clove".into(),
            mode: "Competitive".into(), played_at: 0, kills: 10, deaths: 5,
            assists: 2, score: 3000, rounds: 10, won: Some(true),
            headshots: 10, bodyshots: 30, legshots: 10,
        };
        assert_eq!(m.acs(), 300.0);
        assert_eq!(m.hs_pct(), Some(20.0));
    }
    #[test]
    fn parses_lifetime_history() {
        let payload = serde_json::json!({"data":[{
            "meta":{"id":"test-1","map":{"name":"Ascent"},"mode":"Competitive","started_at":"2026-11-20T12:00:00Z"},
            "stats":{"character":{"name":"Clove"},"team":"Red","kills":17,"deaths":14,"assists":5,"score":4200,
                     "shots":{"head":13,"body":37,"leg":1}},
            "teams":{"red":13,"blue":10}
        }]});
        let matches = parse_lifetime_matches(&payload);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "test-1");
        assert_eq!(matches[0].won, Some(true));
        assert_eq!(matches[0].rounds, 23);
    }

    #[test]
    fn round_results_are_derived_from_actual_winners() {
        let game = serde_json::json!({"rounds":[
            {"winning_team":"Red"},
            {"winning_team":"Blue"},
            {"winning_team":"Unknown"}
        ]});
        assert_eq!(round_winners(&game, "red"), vec![Some(true), Some(false), None]);
    }

}
