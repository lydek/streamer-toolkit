//! The control panel (`fake-services --ui`, `ui` feature): one small iced window beside the app, always on top by
//! default. It runs in the same process as the fakes and reads and drives them directly. Layout: the app's
//! connections on the left, tabs on the right, the activity log at the bottom. Text stays at 14 px or more.

mod faults;
mod pages;
mod send;
mod stats;

use std::time::{Duration, Instant};

use iced::{
    Alignment, Color, Element, Font,
    Length::Fill,
    Size, Subscription, Task, keyboard, mouse, time,
    widget::{button, checkbox, column, container, mouse_area, row, rule, space, table, text},
    window,
};
use serde_json::Value;

use crate::{
    control,
    metrics::{Direction, Entry, Fault},
    obs::{FakeObs, ObsSnapshot},
    twitch::{FakeTwitch, TwitchSnapshot},
};

const TEXT: f32 = 15.0;
const SMALL: f32 = 14.0;
/// Lines kept in the activity log.
const LOG_LINES: usize = 300;
/// The window refreshes this often; the stats report (a full analysis) every fourth time.
const TICK: Duration = Duration::from_millis(250);
const GREEN: Color = Color::from_rgb(0.10, 0.50, 0.22);
const RED: Color = Color::from_rgb(0.71, 0.14, 0.09);
const AMBER: Color = Color::from_rgb(0.60, 0.40, 0.0);
const MUTED: Color = Color::from_rgb(0.27, 0.30, 0.35);
const LINK: Color = Color::from_rgb(0.10, 0.36, 0.75);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Send,
    Obs,
    Twitch,
    Faults,
    Stats,
}

const TABS: [(Tab, &str); 5] = [
    (Tab::Send, "送事件"),
    (Tab::Obs, "假 OBS"),
    (Tab::Twitch, "假 Twitch"),
    (Tab::Faults, "故障"),
    (Tab::Stats, "API 統計"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Twitch,
    Obs,
    Warnings,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Tab(Tab),
    ClearFaults,
    ResetStats,
    OnTop(bool),
    ToggleDock,
    Filter(Filter),
    Pause(bool),
    ClearLog,
    Key(keyboard::Event),
    /// A control body, as the `send` command posts it.
    Send(Value),
    Done(Result<Value, String>),
    Scenario(&'static str),
    Edit(send::Field, String),
    Kind(send::Kind),
    Tier(&'static str),
    Reward(&'static str),
    Status(send::Status),
    First(bool),
    /// A delivered redemption or chat message clicked in the log: the form kind and the ID.
    Pick(&'static str, String),
    DropEventSub,
    DropIrc,
    /// A control command, as `fake-services <words>` would post it.
    Run(Vec<String>),
    /// A scene clicked in the OBS tab (its sources show), or double-clicked (it becomes the program scene).
    Scene(String),
    Program(String),
    StudioMode(bool),
    BurstCount(String),
    FaultKey(String),
    FaultStatus(String),
    FaultDelay(String),
    FaultFor(String),
    FaultClose(bool),
    RemoveFault(String),
    /// A calls-table heading clicked in the stats tab.
    Sort(usize),
}

/// One activity-log line, and what clicking its event fills in.
#[derive(Debug, Clone)]
struct Line {
    cells: [String; 6],
    pick: Option<(&'static str, String)>,
}

pub struct Panel {
    obs: FakeObs,
    twitch: FakeTwitch,
    ports: (u16, u16),
    started: Instant,
    tab: Tab,
    scale: f32,
    on_top: bool,
    dock_open: bool,
    filter: Filter,
    paused: bool,
    /// Lines before this were cleared from the log.
    cleared: Option<Instant>,
    log: Vec<Entry>,
    obs_view: ObsSnapshot,
    twitch_view: TwitchSnapshot,
    report: Value,
    faults: Vec<(String, Fault, Duration)>,
    ticks: u64,
    form: send::Form,
    /// The last send or scenario result, and whether it needs attention.
    notice: Option<(String, bool)>,
    /// The scene whose sources the OBS tab shows; the program scene when none is picked.
    scene: Option<String>,
    burst: String,
    fault_form: faults::FaultForm,
    sort: stats::Sort,
}

/// Opens the panel and blocks until it is closed. The fakes keep running on their own runtime meanwhile.
pub fn run(obs: FakeObs, twitch: FakeTwitch, ports: (u16, u16)) -> iced::Result {
    iced::application(
        move || Panel::new(obs.clone(), twitch.clone(), ports),
        Panel::update,
        Panel::view,
    )
    .title("Fake services")
    .subscription(Panel::subscription)
    .scale_factor(|p: &Panel| p.scale)
    .settings(iced::Settings {
        // From the system, so Chinese renders and no font is bundled.
        default_font: Font::with_name("Microsoft JhengHei UI"),
        default_text_size: TEXT.into(),
        ..Default::default()
    })
    .window_size(Size::new(900.0, 680.0))
    .level(window::Level::AlwaysOnTop)
    .run()
}

impl Panel {
    fn new(obs: FakeObs, twitch: FakeTwitch, ports: (u16, u16)) -> Self {
        let mut panel = Self {
            obs_view: obs.snapshot(),
            twitch_view: twitch.snapshot(),
            report: twitch.recorder().report(),
            obs,
            twitch,
            ports,
            started: Instant::now(),
            tab: Tab::Send,
            scale: 1.0,
            on_top: true,
            dock_open: true,
            filter: Filter::All,
            paused: false,
            cleared: None,
            log: Vec::new(),
            faults: Vec::new(),
            ticks: 0,
            form: send::Form::default(),
            notice: None,
            scene: None,
            burst: "10".into(),
            fault_form: faults::FaultForm::default(),
            sort: stats::Sort::default(),
        };
        panel.refresh();
        panel
    }

    fn refresh(&mut self) {
        self.obs_view = self.obs.snapshot();
        self.twitch_view = self.twitch.snapshot();
        let recorder = self.twitch.recorder();
        self.faults = recorder.faults();
        if self.ticks.is_multiple_of(4) {
            self.report = recorder.report();
        }
        if !self.paused {
            self.log = recorder.recent(LOG_LINES);
        }
        self.ticks += 1;
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => self.refresh(),
            Message::Tab(tab) => self.tab = tab,
            Message::ClearFaults => self.twitch.recorder().clear_faults(),
            Message::ResetStats => {
                self.twitch.recorder().reset();
                self.ticks = 0;
                self.refresh();
            }
            Message::OnTop(on) => {
                self.on_top = on;
                let level = if on {
                    window::Level::AlwaysOnTop
                } else {
                    window::Level::Normal
                };
                return window::latest().and_then(move |id| window::set_level(id, level));
            }
            Message::ToggleDock => self.dock_open = !self.dock_open,
            Message::Filter(filter) => self.filter = filter,
            Message::Pause(paused) => self.paused = paused,
            Message::ClearLog => self.cleared = Some(Instant::now()),
            Message::Key(keyboard::Event::KeyPressed { key, modifiers, .. })
                if modifiers.command() =>
            {
                if let keyboard::Key::Character(c) = key.as_ref() {
                    self.scale = zoom(self.scale, c);
                }
            }
            Message::Key(_) => {}
            Message::Send(body) => {
                self.notice = Some(send::notice(&control::send_body(&self.twitch, &body)))
            }
            Message::Done(result) => self.notice = Some(send::notice(&result)),
            Message::Scenario(name) => {
                self.notice = Some((format!("{name} 執行中"), false));
                let (obs, twitch) = (self.obs.clone(), self.twitch.clone());
                return Task::perform(
                    async move { control::run_scenario(&obs, &twitch, name).await },
                    Message::Done,
                );
            }
            Message::Edit(field, value) => self.form.set(field, value),
            Message::Kind(kind) => self.form.kind = kind.0,
            Message::Tier(tier) => self.form.tier = tier,
            Message::Reward(reward) => self.form.reward = reward,
            Message::Status(status) => self.form.status = status.0,
            Message::First(first) => self.form.first = first,
            Message::Pick(kind, id) => {
                self.form.kind = kind;
                self.form.id = id;
                self.tab = Tab::Send;
            }
            Message::DropEventSub => {
                self.twitch.drop_eventsub();
                self.notice = Some(("已中斷 EventSub".to_owned(), false));
            }
            Message::DropIrc => {
                self.twitch.drop_irc();
                self.notice = Some(("已中斷 IRC".to_owned(), false));
            }
            Message::Run(words) => {
                let result = control::run_command(&self.obs, &self.twitch, &words);
                self.notice = Some(send::notice(&result));
                // At once, so a dragged slider does not jump back until the next tick.
                self.refresh();
            }
            Message::Scene(scene) => self.scene = Some(scene),
            Message::Program(scene) => {
                let result = self.obs.set_program(&scene).map(|()| Value::Null);
                self.notice = Some(send::notice(&result));
                self.scene = Some(scene);
                self.refresh();
            }
            Message::StudioMode(on) => {
                self.obs.set_studio_mode(on);
                self.refresh();
            }
            Message::BurstCount(count) => self.burst = count,
            Message::FaultKey(key) => self.fault_form.key = key,
            Message::FaultStatus(status) => self.fault_form.status = status,
            Message::FaultDelay(delay) => self.fault_form.delay_ms = delay,
            Message::FaultFor(lasts) => self.fault_form.for_s = lasts,
            Message::FaultClose(close) => self.fault_form.close = close,
            Message::RemoveFault(key) => {
                self.twitch.recorder().remove_fault(&key);
                self.refresh();
            }
            Message::Sort(column) => self.sort = self.sort.clicked(column),
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            time::every(TICK).map(|_| Message::Tick),
            keyboard::listen().map(Message::Key),
        ])
    }

    fn view(&self) -> Element<'_, Message> {
        let body = row![self.sidebar(), rule::vertical(1), self.main()].height(Fill);
        column![
            self.title_bar(),
            rule::horizontal(1),
            body,
            rule::horizontal(1),
            self.dock()
        ]
        .into()
    }

    fn title_bar(&self) -> Element<'_, Message> {
        let up = self.started.elapsed().as_secs();
        let meta = format!(
            "OBS :{} · Twitch :{} · {:02}:{:02}",
            self.ports.0,
            self.ports.1,
            up / 60,
            up % 60
        );
        row![
            text("Fake services").size(TEXT),
            space::horizontal(),
            text(meta).size(SMALL).color(MUTED),
            small_button("清除故障", Message::ClearFaults),
            small_button("歸零統計", Message::ResetStats),
            checkbox(self.on_top)
                .label("置頂")
                .text_size(SMALL)
                .on_toggle(Message::OnTop),
        ]
        .spacing(10)
        .padding([6, 12])
        .align_y(Alignment::Center)
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let t = &self.twitch_view;
        let o = &self.obs_view;
        let warnings = self.report["warnings"].as_array().map_or(0, Vec::len);
        let yes_no = |b: bool| if b { "是" } else { "否" };
        let connections = column![
            heading("app 連線"),
            status_row("OBS", o.open),
            status_row("EventSub", t.eventsub_open),
            kv("訂閱", format!("{} 種", t.subscriptions.len())),
            status_row("IRC", t.irc_open),
            kv("登入", crate::twitch::BROADCASTER_LOGIN.to_owned()),
        ]
        .spacing(3);
        let state = column![
            heading("狀態"),
            kv("Twitch 直播中", yes_no(t.live).to_owned()),
            kv("OBS 開播", yes_no(o.streaming).to_owned()),
            kv("OBS 錄影", yes_no(o.recording).to_owned()),
            kv("目前場景", o.program.clone()),
        ]
        .spacing(3);
        let attention = column![
            heading("注意"),
            row![
                text("異常警告"),
                space::horizontal(),
                text(warnings.to_string()).color_maybe((warnings > 0).then_some(AMBER))
            ],
            kv("故障中", self.faults.len().to_string()),
        ]
        .spacing(3);
        container(column![connections, state, attention].spacing(12))
            .width(180)
            .padding([10, 12])
            .into()
    }

    fn main(&self) -> Element<'_, Message> {
        let warnings = self.report["warnings"].as_array().map_or(0, Vec::len);
        let tabs = row(TABS.iter().map(|(tab, label)| {
            let label = if *tab == Tab::Stats && warnings > 0 {
                format!("{label} {warnings}")
            } else {
                (*label).to_owned()
            };
            let b = button(text(label).size(TEXT))
                .padding([5, 10])
                .on_press(Message::Tab(*tab));
            let b = if *tab == self.tab {
                b.style(button::primary)
            } else {
                b.style(button::text)
            };
            b.into()
        }))
        .spacing(2)
        .padding([6, 10]);
        let page: Element<'_, Message> = match self.tab {
            Tab::Send => self.send_page(),
            Tab::Twitch => self.twitch_page(),
            Tab::Obs => self.obs_page(),
            Tab::Faults => self.faults_page(),
            Tab::Stats => self.stats_page(),
        };
        let notice = self.notice.as_ref().map(|(line, warn)| {
            container(
                text(line.clone())
                    .size(SMALL)
                    .color(if *warn { AMBER } else { GREEN }),
            )
            .padding([2, 12])
        });
        column![tabs, rule::horizontal(1), page]
            .push(notice)
            .width(Fill)
            .into()
    }

    fn dock(&self) -> Element<'_, Message> {
        let arrow = if self.dock_open {
            "▾ 活動紀錄"
        } else {
            "▸ 活動紀錄"
        };
        let filter = |f: Filter, label: &'static str| {
            let b = small_button(label, Message::Filter(f));
            if self.filter == f {
                b.style(button::primary)
            } else {
                b
            }
        };
        let head = row![
            button(text(arrow).size(TEXT))
                .style(button::text)
                .padding(0)
                .on_press(Message::ToggleDock),
            space::horizontal(),
            filter(Filter::All, "全部"),
            filter(Filter::Twitch, "Twitch"),
            filter(Filter::Obs, "OBS"),
            filter(Filter::Warnings, "警告"),
            checkbox(self.paused)
                .label("暫停")
                .text_size(SMALL)
                .on_toggle(Message::Pause),
            small_button("清空", Message::ClearLog),
        ]
        .spacing(6)
        .padding([4, 12])
        .align_y(Alignment::Center);
        if !self.dock_open {
            return head.into();
        }
        let lines: Vec<Line> = visible(&self.log, self.filter, self.cleared)
            .rev()
            .map(|e| Line {
                cells: cells(e, self.started),
                pick: pick(e),
            })
            .collect();
        let col = |title: &'static str, i: usize| {
            table::column(
                text(title).size(SMALL).color(MUTED),
                move |line: Line| -> Element<'_, Message> {
                    let c = &line.cells;
                    // Monospace only for plain ASCII: it has no Chinese glyphs, and the fallback renders blurry.
                    let cell = text(c[i].clone()).size(SMALL);
                    let cell = if c[i].is_ascii() {
                        cell.font(Font::MONOSPACE)
                    } else {
                        cell
                    };
                    let cell = match (i, c[2].as_str(), c[4].as_str()) {
                        (2, "OUT", _) => cell.color(Color::from_rgb(0.36, 0.29, 0.86)),
                        (4, _, s) if s.starts_with('5') || s == "未送出" => cell.color(RED),
                        _ => cell,
                    };
                    match line.pick {
                        // Clicking a redemption or chat message fills its update or deletion in the send tab.
                        Some((kind, id)) if i == 3 => mouse_area(cell.color(LINK))
                            .interaction(mouse::Interaction::Pointer)
                            .on_press(Message::Pick(kind, id))
                            .into(),
                        _ => cell.into(),
                    }
                },
            )
        };
        let log = table::table(
            [
                col("秒", 0),
                col("服務", 1),
                col("方向", 2),
                col("呼叫或事件", 3).width(Fill),
                col("狀態", 4),
                col("ms", 5),
            ],
            lines,
        )
        .padding_x(8)
        .padding_y(2);
        column![head, container(scrollable(log).height(Fill)).height(140)].into()
    }
}

/// The new scale after Ctrl with `key`: `=`/`+` larger, `-` smaller, `0` back to 100 %; 75 % to 200 %.
pub fn zoom(scale: f32, key: &str) -> f32 {
    let next = match key {
        "=" | "+" => scale + 0.1,
        "-" => scale - 0.1,
        "0" => 1.0,
        _ => return scale,
    };
    (next * 10.0).round().clamp(7.5, 20.0) / 10.0
}

/// Log entries shown for a filter, after the last clear.
pub fn visible(
    log: &[Entry],
    filter: Filter,
    cleared: Option<Instant>,
) -> impl DoubleEndedIterator<Item = &Entry> {
    log.iter().filter(move |e| {
        cleared.is_none_or(|c| e.at > c)
            && match filter {
                Filter::All => true,
                Filter::Twitch => e.service == "twitch",
                Filter::Obs => e.service == "obs",
                // A failed call or an event that never went out.
                // OBS's 600s answer about the resource (604: no audio), as in `metrics`; not a failure.
                Filter::Warnings => {
                    !e.delivered
                        || e.status.is_some_and(|s| {
                            s >= 400 && !(e.service == "obs" && (600..700).contains(&s))
                        })
                }
            }
    })
}

/// What clicking a log line fills in: only a delivered redemption or chat message that has an ID.
fn pick(e: &Entry) -> Option<(&'static str, String)> {
    let id =
        e.id.as_ref()
            .filter(|_| e.delivered && e.direction == Direction::Out)?;
    Some((send::pick_kind(&e.key)?, id.clone()))
}

/// One log line: seconds since the panel opened, service, direction, key (with the ID), status, ms.
pub fn cells(e: &Entry, started: Instant) -> [String; 6] {
    let secs = e.at.saturating_duration_since(started).as_secs_f64();
    let key = match &e.id {
        Some(id) => format!("{} {id}", e.key),
        None => e.key.clone(),
    };
    let (dir, status) = match e.direction {
        Direction::In => ("IN", e.status.map(|s| s.to_string()).unwrap_or_default()),
        Direction::Out if e.delivered => ("OUT", "已送出".to_owned()),
        Direction::Out => ("OUT", "未送出".to_owned()),
    };
    let ms = e
        .handled
        .map(|d| format!("{:.1}", d.as_secs_f64() * 1000.0))
        .unwrap_or_default();
    [
        format!("{secs:.1}"),
        e.service.to_owned(),
        dir.to_owned(),
        key,
        status,
        ms,
    ]
}

fn heading(label: &str) -> Element<'_, Message> {
    text(label).size(SMALL).color(MUTED).into()
}

fn kv(label: &str, value: String) -> Element<'_, Message> {
    row![text(label), space::horizontal(), text(value)].into()
}

fn status_row(label: &str, open: u32) -> Element<'_, Message> {
    let (dot, value) = if open > 0 {
        (GREEN, format!("{open} 條"))
    } else {
        (RED, "未連線".to_owned())
    };
    row![
        text("● ").color(dot),
        text(label),
        space::horizontal(),
        text(value)
    ]
    .into()
}

/// A scrollable whose scrollbar sits beside the content instead of covering its edge.
fn scrollable<'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Scrollable<'a, Message> {
    iced::widget::scrollable(content).spacing(4)
}

fn small_button(label: &str, message: Message) -> button::Button<'_, Message> {
    button(text(label).size(SMALL))
        .padding([3, 8])
        .style(button::secondary)
        .on_press(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        service: &'static str,
        direction: Direction,
        status: Option<u16>,
        delivered: bool,
        at: Instant,
    ) -> Entry {
        Entry {
            at,
            service,
            direction,
            key: "k".into(),
            status,
            handled: status.map(|_| Duration::from_micros(1500)),
            id: None,
            delivered,
        }
    }

    #[test]
    fn zoom_steps_and_stays_in_range() {
        assert_eq!(zoom(1.0, "="), 1.1);
        assert_eq!(zoom(1.0, "-"), 0.9);
        assert_eq!(zoom(1.7, "0"), 1.0);
        assert_eq!(zoom(2.0, "+"), 2.0);
        assert_eq!(zoom(0.75, "-"), 0.75);
        assert_eq!(zoom(1.0, "a"), 1.0);
    }

    #[test]
    fn filters_pick_services_and_problems_after_a_clear() {
        let t0 = Instant::now();
        let log = vec![
            entry("twitch", Direction::In, Some(200), true, t0),
            entry(
                "obs",
                Direction::In,
                Some(100),
                true,
                t0 + Duration::from_millis(1),
            ),
            entry(
                "twitch",
                Direction::Out,
                None,
                false,
                t0 + Duration::from_millis(2),
            ),
            entry(
                "twitch",
                Direction::In,
                Some(503),
                true,
                t0 + Duration::from_millis(3),
            ),
        ];
        let count = |f, c| visible(&log, f, c).count();
        assert_eq!(count(Filter::All, None), 4);
        assert_eq!(count(Filter::Twitch, None), 3);
        assert_eq!(count(Filter::Obs, None), 1);
        assert_eq!(
            count(Filter::Warnings, None),
            2,
            "the undelivered event and the 503"
        );
        assert_eq!(count(Filter::All, Some(t0 + Duration::from_millis(1))), 2);
        let quiet = [entry("obs", Direction::In, Some(604), true, t0)];
        assert_eq!(
            visible(&quiet, Filter::Warnings, None).count(),
            0,
            "OBS 604 is an answer"
        );
    }

    #[test]
    fn cells_show_direction_status_and_id() {
        let t0 = Instant::now();
        let call = entry(
            "obs",
            Direction::In,
            Some(100),
            true,
            t0 + Duration::from_millis(1500),
        );
        assert_eq!(
            cells(&call, t0),
            ["1.5", "obs", "IN", "k", "100", "1.5"].map(String::from)
        );
        let mut event = entry("twitch", Direction::Out, None, false, t0);
        event.id = Some("fake-3".into());
        let c = cells(&event, t0);
        assert_eq!(
            (c[2].as_str(), c[3].as_str(), c[4].as_str()),
            ("OUT", "k fake-3", "未送出")
        );
    }

    fn panel() -> Panel {
        Panel::new(
            FakeObs::new(None),
            FakeTwitch::new(Duration::from_secs(10)),
            (0, 0),
        )
    }

    fn run(p: &mut Panel, words: &[&str]) {
        let _ = p.update(Message::Run(
            words.iter().map(|w| (*w).to_owned()).collect(),
        ));
    }

    #[test]
    fn obs_controls_drive_the_fake_obs() {
        let mut p = panel();
        let other = p
            .obs_view
            .scenes
            .iter()
            .find(|s| s.name != p.obs_view.program)
            .unwrap()
            .name
            .clone();
        let _ = p.update(Message::Program(other.clone()));
        assert_eq!(p.obs_view.program, other);
        let _ = p.update(Message::Program("no such scene".into()));
        assert!(p.notice.as_ref().unwrap().1, "a failed switch is flagged");
        let _ = p.update(Message::StudioMode(true));
        assert!(p.obs_view.studio_mode);
        run(&mut p, &["obs", "stream", "on"]);
        assert!(p.obs_view.streaming);
        run(&mut p, &["obs", "stats", "--cpu", "42", "--dropped", "6.5"]);
        assert_eq!(
            (
                p.obs_view.stats.cpu_percent,
                p.obs_view.stats.dropped_percent
            ),
            (42.0, 6.5)
        );
        run(&mut p, &["obs", "drop"]);
        assert!(!p.notice.as_ref().unwrap().1);
    }

    #[test]
    fn faults_are_set_and_removed_one_by_one() {
        let mut p = panel();
        for key in ["GET /helix/users", "ws-connect obs"] {
            let _ = p.update(Message::FaultKey(key.into()));
            let words = p.fault_form.words();
            let _ = p.update(Message::Run(words));
        }
        assert_eq!(p.faults.len(), 2);
        assert!(p.faults.iter().all(|(_, f, _)| f.status == Some(503)));
        let _ = p.update(Message::RemoveFault("ws-connect obs".into()));
        assert_eq!(p.faults.len(), 1);
        assert_eq!(p.faults[0].0, "GET /helix/users");
        let _ = p.update(Message::FaultKey(String::new()));
        let words = p.fault_form.words();
        let _ = p.update(Message::Run(words));
        assert!(
            p.notice.as_ref().unwrap().1,
            "a fault without a key is refused"
        );
    }

    #[test]
    fn the_stats_tab_shows_the_report_until_reset() {
        let mut p = panel();
        p.twitch.recorder().call(crate::metrics::Call {
            at: Instant::now(),
            service: "twitch",
            key: "GET /helix/users".into(),
            params: 0,
            status: 401,
            handled: Duration::from_millis(2),
            in_flight: 1,
            problem: Some("no Client-Id header".into()),
        });
        p.ticks = 0;
        let _ = p.update(Message::Tick);
        let rows = stats::sorted_calls(&p.report, p.sort);
        assert_eq!(
            rows,
            p.report["calls"].as_array().unwrap().clone(),
            "the report's rows as they are"
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(p.report["warnings"][0]["kind"], "headers");
        let _ = p.update(Message::ResetStats);
        let rows = stats::sorted_calls(&p.report, p.sort);
        assert!(rows.is_empty());
        assert_eq!(p.report["warnings"], serde_json::json!([]));
    }
}
