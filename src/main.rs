use eframe::egui;
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
}

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
        .query(&[("size", "10")])
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
    Settings,
}

struct Recon {
    page: Page,
    settings: Settings,
    key_input: String,
    matches: Vec<Match>,
    selected: Option<String>,
    worker: Option<Receiver<Result<usize, String>>>,
    status: String,
}

impl Recon {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
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
        ui.heading("Overview");
        ui.label(format!("{}#{}", self.settings.name, self.settings.tag));
        ui.add_space(12.0);
        if self.matches.is_empty() {
            ui.label("No cached matches yet. Open Settings, enter your key, then Sync.");
            return;
        }
        let played = self.matches.len();
        let wins = self.matches.iter().filter(|m| m.won == Some(true)).count();
        let decided = self.matches.iter().filter(|m| m.won.is_some()).count();
        let kills: i64 = self.matches.iter().map(|m| m.kills).sum();
        let deaths: i64 = self.matches.iter().map(|m| m.deaths).sum();
        let assists: i64 = self.matches.iter().map(|m| m.assists).sum();
        let rounds: i64 = self.matches.iter().map(|m| m.rounds).sum();
        let score: i64 = self.matches.iter().map(|m| m.score).sum();
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [
                ("Matches", played.to_string()),
                ("Win rate", if decided > 0 { format!("{:.1}%", wins as f64 * 100.0 / decided as f64) } else { "N/A".into() }),
                ("K / D / A", format!("{kills} / {deaths} / {assists}")),
                ("K/D", format!("{:.2}", kills as f64 / deaths.max(1) as f64)),
                ("ACS", if rounds > 0 { format!("{:.0}", score as f64 / rounds as f64) } else { "N/A".into() }),
            ] {
                ui.group(|ui| {
                    ui.label(label);
                    ui.heading(value);
                });
            }
        });
        ui.add_space(20.0);
        ui.heading("Recent performance");
        let points: Vec<[f64; 2]> = self.matches.iter().rev().enumerate()
            .map(|(idx, m)| [idx as f64, m.acs()]).collect();
        egui_plot::Plot::new("acs_trend").height(200.0).show(ui, |plot| {
            plot.line(egui_plot::Line::new(egui_plot::PlotPoints::from(points)).name("ACS"));
        });
        ui.label("ACS calculated as player score / rounds. Win rate excludes matches with unknown outcome.");
    }

    fn match_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("Match history");
        ui.label("Most recent cached matches. Select one to view available stats.");
        ui.add_space(10.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for m in &self.matches {
                let label = format!(
                    "{} | {} | {} | {} | {}/{}/{} | ACS {:.0}",
                    match m.won { Some(true) => "WIN ", Some(false) => "LOSS", None => "N/A " },
                    m.map, m.agent, m.mode, m.kills, m.deaths, m.assists, m.acs()
                );
                if ui.selectable_label(self.selected.as_deref() == Some(&m.id), label).clicked() {
                    self.selected = Some(m.id.clone());
                }
            }
        });
        if let Some(m) = self.matches.iter().find(|m| Some(m.id.as_str()) == self.selected.as_deref()) {
            ui.separator();
            ui.heading(format!("{} · {}", m.map, m.agent));
            ui.label(format!("Match ID: {}", m.id));
            ui.label(format!("K/D/A: {}/{}/{} | ACS: {:.1}", m.kills, m.deaths, m.assists, m.acs()));
            ui.label(match m.hs_pct() {
                Some(value) => format!("Headshot share of recorded hits: {value:.1}%"),
                None => "Headshot share unavailable".into()
            });
            ui.label("Round timelines and detailed fight analysis are planned for a later version.");
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings · API & Data");
        ui.label("HenrikDev API key is stored in your operating system's credential store.");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label("Riot name");
            ui.text_edit_singleline(&mut self.settings.name);
            ui.label("Tag");
            ui.text_edit_singleline(&mut self.settings.tag);
        });
        ui.horizontal(|ui| {
            ui.label("Region");
            egui::ComboBox::from_id_salt("region")
                .selected_text(&self.settings.region)
                .show_ui(ui, |ui| {
                    for region in ["ap", "eu", "na", "kr", "latam", "br"] {
                        ui.selectable_value(&mut self.settings.region, region.into(), region);
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("API key");
            ui.add(egui::TextEdit::singleline(&mut self.key_input).password(true).hint_text("Paste HenrikDev key"));
        });
        if ui.button("Save settings").clicked() {
            self.save();
        }
        ui.separator();
        ui.label(format!("Local storage: {}", app_dir().display()));
        ui.label("No background tracking, cloud sync or opponent scouting.");
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
                            format!("Synced {count} recent matches.")
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
        egui::TopBottomPanel::top("nav").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("RECON");
                ui.separator();
                ui.selectable_value(&mut self.page, Page::Overview, "Overview");
                ui.selectable_value(&mut self.page, Page::Matches, "Matches");
                ui.selectable_value(&mut self.page, Page::Settings, "Settings");
                ui.separator();
                if ui.add_enabled(self.worker.is_none(), egui::Button::new("Sync matches")).clicked() {
                    self.sync();
                }
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.page {
            Page::Overview => self.overview(ui),
            Page::Matches => self.match_list(ui),
            Page::Settings => self.settings_ui(ui),
        });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "RECON · VALORANT Match Tracker",
        eframe::NativeOptions::default(),
        Box::new(|cc| Ok(Box::new(Recon::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_empty_api_data() {
        let settings = Settings { name: "Me".into(), tag: "TAG".into(), region: "ap".into() };
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
