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

fn fetch_matches(settings: &Settings, key: &str) -> Result<Vec<Match>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "https://api.henrikdev.xyz/valorant/v3/matches/{}/{}/{}",
        urlencoding::encode(settings.region.trim()),
        urlencoding::encode(settings.name.trim()),
        urlencoding::encode(settings.tag.trim())
    );
    let response = client
        .get(url)
        .header("Authorization", key)
        .query(&[("size", settings.sync_count.clamp(5, 50).to_string())])
        .send()
        .map_err(|e| format!("Network request failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HenrikDev returned HTTP {status}. Check your key, Riot ID, region and API limits."));
    }
    let payload: Value = response.json().map_err(|e| format!("Invalid API response: {e}"))?;
    if payload.get("data").and_then(Value::as_array).is_none() {
        return Err("HenrikDev returned no match list; endpoint or response format may have changed.".into());
    }
    Ok(parse_matches(&payload, settings))
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Page {
    Overview,
    Matches,
    Analytics,
    Settings,
}

struct Recon {
    page: Page,
    settings: Settings,
    key_input: String,
    matches: Vec<Match>,
    selected: Option<String>,
    mode_filter: String,
    search: String,
    sidebar_collapsed: bool,
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
        heading(ui, "Overview", "Your recent match performance, stored locally");
        if self.matches.is_empty() {
            ui.label("No matches saved yet. Add your Riot ID and API key in Settings, then sync.");
            if ui.button("Open Settings").clicked() { self.page = Page::Settings; }
            return;
        }
        let round_matches: Vec<&Match> = self.matches.iter().filter(|m| is_round_mode(&m.mode)).collect();
        let wins = round_matches.iter().filter(|m| m.won == Some(true)).count();
        let decided = round_matches.iter().filter(|m| m.won.is_some()).count();
        let kills: i64 = round_matches.iter().map(|m| m.kills).sum();
        let deaths: i64 = round_matches.iter().map(|m| m.deaths).sum();
        let scored: i64 = round_matches.iter().map(|m| m.score).sum();
        let rounds: i64 = round_matches.iter().map(|m| m.rounds).sum();
        ui.horizontal_wrapped(|ui| {
            metric(ui, "Match wins", &format!("{wins} / {decided}"), "Round-based modes");
            metric(ui, "Win rate", &if decided == 0 { "N/A".into() } else {format!("{:.0}%", 100.0 * wins as f64 / decided as f64)}, "Completed matches");
            metric(ui, "K / D", &format!("{:.2}", kills as f64 / deaths.max(1) as f64), "All round-based matches");
            metric(ui, "Average ACS", &if rounds == 0 { "N/A".into() } else {format!("{:.0}", scored as f64 / rounds as f64)}, "Weighted by rounds");
        });
        ui.add_space(16.0);
        egui::Frame::new().fill(PANEL).corner_radius(9.0).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
            ui.label(RichText::new("ACS TREND").color(WHITE).strong());
            ui.label(RichText::new("Chronological · round-based modes only").color(MUTED).size(11.0));
            let points: Vec<[f64; 2]> = round_matches.iter().rev().filter(|m| m.rounds > 0)
                .enumerate().map(|(idx, m)| [idx as f64, m.acs()]).collect();
            if points.is_empty() { ui.label("No supported round-based matches."); }
            else {
                egui_plot::Plot::new("acs_dashboard").height(190.0).show(ui, |plot| {
                    plot.line(egui_plot::Line::new(egui_plot::PlotPoints::from(points)).name("ACS").color(RED).width(2.5));
                });
            }
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("RECENT MATCHES").color(WHITE).strong());
            if ui.button("View all").clicked() { self.page = Page::Matches; }
        });
        let recents: Vec<Match> = self.matches.iter().take(5).cloned().collect();
        for m in &recents { self.match_card(ui, m); }
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
            ui.add_space(12.0);
            heading(ui, &format!("{}  /  {}", m.map, m.agent), &format!("{} · Match summary", m.mode));
            let (outcome, color) = outcome_label(m.won);
            ui.colored_label(color, RichText::new(outcome).strong().size(15.0));
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                metric(ui, "Kills", &m.kills.to_string(), "This match");
                metric(ui, "Deaths", &m.deaths.to_string(), "This match");
                metric(ui, "Assists", &m.assists.to_string(), "This match");
                if is_round_mode(&m.mode) && m.rounds > 0 {
                    metric(ui, "ACS", &format!("{:.0}", m.acs()), "Round-based only");
                }
            });
            ui.add_space(14.0);
            egui::Frame::new().fill(PANEL).corner_radius(8.0).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                ui.label(RichText::new("COMBAT BREAKDOWN").color(WHITE).strong());
                ui.label(match m.hs_pct() {
                    Some(v) => format!("Headshots as share of registered hits: {v:.1}%"),
                    None => "Hit distribution unavailable".to_owned(),
                });
                ui.label(format!("Head: {}  ·  Body: {}  ·  Legs: {}", m.headshots, m.bodyshots, m.legshots));
                if is_round_mode(&m.mode) && m.rounds > 0 {
                    ui.label(format!("Rounds played: {}", m.rounds));
                } else {
                    ui.label(RichText::new("ACS is not calculated for this mode.").color(MUTED));
                }
                ui.separator();
                ui.label(RichText::new("Round-by-round analysis will come with richer match data.").color(MUTED));
                ui.label(RichText::new(format!("Match ID: {}", m.id)).color(MUTED).size(10.0));
            });
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
        heading(ui, "Analytics", "Agent and map breakdowns from cached round-based matches");
        let supported: Vec<&Match> = self.matches.iter().filter(|m| is_round_mode(&m.mode)).collect();
        if supported.is_empty() { ui.label("No supported matches to analyze yet."); return; }
        for (title, by_agent) in [("AGENT PERFORMANCE", true), ("MAP PERFORMANCE", false)] {
            egui::Frame::new().fill(PANEL).corner_radius(8.0).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                ui.label(RichText::new(title).color(WHITE).strong());
                let mut keys: Vec<String> = supported.iter().map(|m| if by_agent { m.agent.clone() } else { m.map.clone() }).collect();
                keys.sort();
                keys.dedup();
                egui::Grid::new(title).num_columns(4).spacing([24.0, 9.0]).striped(true).show(ui, |ui| {
                    for h in ["", "Played", "Wins", "Win rate"] { ui.label(RichText::new(h).color(MUTED).strong()); }
                    ui.end_row();
                    for key in keys {
                        let group: Vec<&&Match> = supported.iter().filter(|m| if by_agent { m.agent == key } else { m.map == key }).collect();
                        let decided = group.iter().filter(|m| m.won.is_some()).count();
                        let wins = group.iter().filter(|m| m.won == Some(true)).count();
                        ui.label(&key);
                        ui.label(group.len().to_string());
                        ui.label(wins.to_string());
                        ui.label(if decided > 0 { format!("{:.0}%", wins as f64 * 100.0 / decided as f64) } else { "N/A".to_string() });
                        ui.end_row();
                    }
                });
            });
            ui.add_space(16.0);
        }
        ui.label(RichText::new("Small samples can be misleading. Match count is shown for context.").color(MUTED).size(11.0));
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
            ui.label(RichText::new("The provider may return fewer than requested. Existing matches remain cached.").color(MUTED).size(11.0));
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
}
