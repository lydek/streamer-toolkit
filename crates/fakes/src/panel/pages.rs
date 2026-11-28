//! The tab pages.

use iced::{
    Alignment, Element, Font,
    Length::{Fill, FillPortion, Shrink},
    widget::{
        button, checkbox, column, container, mouse_area, pick_list, progress_bar, row, slider,
        table, text, text_input,
    },
};

use super::{
    AMBER, GREEN, MUTED, Message, Panel, RED, SMALL, TEXT, heading, kv, scrollable,
    send::{self, Field, Form, Kind, STATUSES, Status, TIERS},
    small_button,
};
use crate::metrics::Fault;
use crate::twitch::{BROADCASTER_ID, BROADCASTER_LOGIN, RewardView};
use serde_json::Value;

const SCENARIOS: [(&str, &str); 4] = [
    ("stream-start", "開播流程"),
    ("stream-end", "下播流程"),
    ("gift-bomb", "送禮 ×20"),
    ("redemption-flood", "兌換洪水 ×30"),
];

impl Panel {
    pub(super) fn send_page(&self) -> Element<'_, Message> {
        let form = &self.form;
        let quick = |range: std::ops::Range<usize>| {
            row(send::QUICK[range].iter().map(|(kind, label)| {
                button(text(*label).size(TEXT).center())
                    .width(Fill)
                    .padding([5, 6])
                    .style(button::secondary)
                    .on_press(Message::Send(form.body_for(kind)))
                    .into()
            }))
            .spacing(6)
        };
        let t = &self.twitch_view;
        let redemption = t.last_redemption.clone();
        let update = |status: &'static str| {
            redemption.as_ref().map(|id| {
                Message::Send(
                    Form {
                        kind: "redeem-update",
                        id: id.clone(),
                        status,
                        ..form.clone()
                    }
                    .body(),
                )
            })
        };
        let delete = t.last_chat.as_ref().map(|id| {
            Message::Send(
                Form {
                    kind: "delete",
                    id: id.clone(),
                    ..form.clone()
                }
                .body(),
            )
        });
        let card =
            |title: &'static str, id: &Option<String>, buttons: Vec<Element<'static, Message>>| {
                container(
                    column![
                        heading(title),
                        row![text(id.clone().unwrap_or_else(|| "還沒有".into())).width(Fill)]
                            .push(row(buttons).spacing(6))
                            .align_y(Alignment::Center),
                    ]
                    .spacing(4),
                )
                .style(container::bordered_box)
                .padding([6, 10])
                .width(Fill)
            };
        let action = |label: &'static str, message: Option<Message>| -> Element<'static, Message> {
            button(text(label).size(SMALL))
                .padding([3, 8])
                .style(button::secondary)
                .on_press_maybe(message)
                .into()
        };
        let cards = row![
            card(
                "最後兌換",
                &t.last_redemption,
                vec![
                    action("完成", update("fulfilled")),
                    action("取消", update("canceled"))
                ]
            ),
            card("最後聊天", &t.last_chat, vec![action("刪除", delete)]),
        ]
        .spacing(8);

        let kinds: Vec<Kind> = send::KINDS.iter().map(|(k, _)| Kind(k)).collect();
        let mut fields = row![labeled(
            "事件",
            pick_list(kinds, Some(Kind(form.kind)), Message::Kind)
                .text_size(TEXT)
                .into()
        )]
        .spacing(8)
        .align_y(Alignment::End);
        for field in Field::ALL.into_iter().filter(|f| f.used_by(form.kind)) {
            let input: Element<'_, Message> = match field {
                Field::Tier => pick_list(&TIERS[..], Some(form.tier), Message::Tier)
                    .text_size(TEXT)
                    .into(),
                Field::Reward => {
                    let titles: Vec<String> = self
                        .twitch_view
                        .rewards
                        .iter()
                        .map(|r| r.title.clone())
                        .collect();
                    pick_list(titles, Some(form.reward.clone()), Message::Reward)
                        .text_size(TEXT)
                        .into()
                }
                Field::Status => {
                    pick_list(&STATUSES[..], Some(Status(form.status)), Message::Status)
                        .text_size(TEXT)
                        .into()
                }
                _ => {
                    let width = match field {
                        Field::Count | Field::Bits | Field::Months => 70.into(),
                        Field::Message => Fill,
                        _ => 140.into(),
                    };
                    text_input("", form.value(field))
                        .on_input(move |v| Message::Edit(field, v))
                        .size(TEXT)
                        .width(width)
                        .into()
                }
            };
            fields = fields.push(labeled(field.label(), input));
        }
        if form.kind == "chat" {
            fields = fields.push(
                checkbox(form.first)
                    .label("首次聊天")
                    .text_size(SMALL)
                    .on_toggle(Message::First),
            );
        }
        let fields = fields.push(
            button(text("送出").size(TEXT))
                .padding([5, 14])
                .style(button::primary)
                .on_press(Message::Send(form.body())),
        );

        let scenarios = row(SCENARIOS
            .iter()
            .map(|(name, label)| small_button(label, Message::Scenario(name)).into()))
        .spacing(6);
        scrollable(
            column![
                heading("快速送出"),
                quick(0..6),
                quick(6..12),
                cards,
                heading("自訂事件"),
                fields,
                heading("情境"),
                scenarios,
            ]
            .spacing(8)
            .padding([10, 12]),
        )
        .height(Fill)
        .into()
    }

    pub(super) fn twitch_page(&self) -> Element<'_, Message> {
        let t = &self.twitch_view;
        let channels = if t.irc_joins.is_empty() {
            "無".to_owned()
        } else {
            t.irc_joins.join("、")
        };
        let account = column![
            kv("帳號", format!("{BROADCASTER_LOGIN} · {BROADCASTER_ID}")),
            kv("直播中", if t.live { "是" } else { "否" }.to_owned()),
            kv("標題", t.channel.title.clone()),
            kv("分類", t.channel.category_name().to_owned()),
            kv("IRC 頻道", channels),
        ]
        .spacing(3)
        .width(280);
        let drops = column![
            small_button("中斷 EventSub", Message::DropEventSub),
            small_button("中斷 IRC", Message::DropIrc),
        ]
        .spacing(6);
        let rewards = table::table(
            [
                table::column(heading("獎勵"), |r: RewardView| text(r.title)).width(Fill),
                table::column(heading("點數"), |r: RewardView| text(r.cost.to_string())),
                table::column(heading("app 建立"), |r: RewardView| {
                    text(if r.app_created { "是" } else { "" })
                }),
                table::column(heading("ID"), |r: RewardView| {
                    text(r.id).size(SMALL).color(MUTED)
                }),
            ],
            t.rewards.clone(),
        )
        .padding_x(8)
        .padding_y(2);
        let subscriptions = column(
            t.subscriptions
                .iter()
                .map(|s| text(s.clone()).size(SMALL).into()),
        )
        .spacing(2);
        column![
            row![account, drops].spacing(24),
            self.poll_row(),
            self.prediction_row(),
            row![
                column![rewards].width(FillPortion(3)),
                column![
                    text(format!("app 建立的訂閱（{}）", t.subscriptions.len()))
                        .size(SMALL)
                        .color(MUTED),
                    scrollable(subscriptions).height(Fill)
                ]
                .spacing(4)
                .width(FillPortion(2)),
            ]
            .spacing(16)
            .height(Fill),
        ]
        .spacing(12)
        .padding([10, 12])
        .into()
    }
}

impl Panel {
    /// The latest poll: each choice's votes (a click adds one while it runs), and ending it now.
    fn poll_row(&self) -> Element<'_, Message> {
        let Some(p) = &self.twitch_view.poll else {
            return row![heading("投票"), text("沒有").color(MUTED)]
                .spacing(8)
                .into();
        };
        let active = p.status == "ACTIVE";
        let status = match p.status.as_str() {
            "ACTIVE" => "進行中",
            "COMPLETED" => "已結束",
            "TERMINATED" => "提早結束",
            _ => "已封存",
        };
        let mut line = row![heading("投票"), text(format!("{} · {status}", p.title))]
            .spacing(8)
            .align_y(Alignment::Center);
        for (n, (title, votes)) in p.choices.iter().enumerate() {
            let label = text(format!("{title} {votes} 票")).size(SMALL);
            line = line.push(if active {
                let choice = (n + 1).to_string();
                button(label)
                    .padding([3, 8])
                    .style(button::secondary)
                    .on_press(command(&["poll", "vote", "--choice", &choice]))
                    .into()
            } else {
                Element::from(label)
            });
        }
        if active {
            line = line.push(small_button("結束", command(&["poll", "end"])));
        }
        line.wrap().into()
    }

    /// The latest prediction: each outcome's predictors and points (a click bets 100 points while it is open), and
    /// locking it now. A result or a cancel comes from the app, as on Twitch.
    fn prediction_row(&self) -> Element<'_, Message> {
        let Some(p) = &self.twitch_view.prediction else {
            return row![heading("預測"), text("沒有").color(MUTED)]
                .spacing(8)
                .into();
        };
        let open = p.status == "ACTIVE";
        let status = match p.status.as_str() {
            "ACTIVE" => "下注中",
            "LOCKED" => "已鎖定",
            "RESOLVED" => "已結算",
            _ => "已取消",
        };
        let mut line = row![heading("預測"), text(format!("{} · {status}", p.title))]
            .spacing(8)
            .align_y(Alignment::Center);
        for (n, (title, users, points)) in p.outcomes.iter().enumerate() {
            let label = text(format!("{title} {users} 人 {points} 點")).size(SMALL);
            line = line.push(if open {
                let outcome = (n + 1).to_string();
                button(label)
                    .padding([3, 8])
                    .style(button::secondary)
                    .on_press(command(&["prediction", "bet", "--outcome", &outcome]))
                    .into()
            } else {
                Element::from(label)
            });
        }
        if open {
            line = line.push(small_button("鎖定", command(&["prediction", "lock"])));
        }
        line.wrap().into()
    }

    pub(super) fn obs_page(&self) -> Element<'_, Message> {
        let o = &self.obs_view;
        let selected = self.scene.as_deref().unwrap_or(&o.program);
        let scenes = column(o.scenes.iter().map(|scene| {
            let mark = if scene.name == o.program {
                text("節目").size(SMALL).color(GREEN)
            } else if o.preview.as_deref() == Some(scene.name.as_str()) {
                text("預覽").size(SMALL).color(MUTED)
            } else {
                text("")
            };
            let line = container(row![text(scene.name.clone()).width(Fill), mark].spacing(6))
                .padding([3, 6])
                .width(Fill);
            let line = if scene.name == selected {
                line.style(container::rounded_box)
            } else {
                line
            };
            mouse_area(line)
                .on_press(Message::Scene(scene.name.clone()))
                .on_double_click(Message::Program(scene.name.clone()))
                .into()
        }))
        .spacing(2);
        let items = o
            .scenes
            .iter()
            .find(|s| s.name == selected)
            .map(|s| s.items.clone())
            .unwrap_or_default();
        let yes = |b: bool| if b { "✓" } else { "—" };
        let sources = table::table(
            [
                table::column(heading("來源"), |i: crate::obs::ItemView| text(i.source))
                    .width(Fill),
                table::column(heading("顯示"), move |i: crate::obs::ItemView| {
                    text(yes(i.enabled))
                }),
                table::column(heading("鎖定"), move |i: crate::obs::ItemView| {
                    text(yes(i.locked))
                }),
            ],
            items,
        )
        .padding_x(6)
        .padding_y(2);

        let output = |label: &'static str, on: bool, action: &'static str| {
            checkbox(on)
                .label(label)
                .text_size(TEXT)
                .on_toggle(move |on| command(&["obs", action, if on { "on" } else { "off" }]))
        };
        let toggles = row![
            output("開播", o.streaming, "stream"),
            output("錄影", o.recording, "record"),
            checkbox(o.studio_mode)
                .label("工作室模式")
                .text_size(TEXT)
                .on_toggle(Message::StudioMode),
        ]
        .spacing(12);
        let count = self.burst.trim().to_owned();
        let burst = |label: &'static str, kind: &'static str| {
            small_button(label, command(&["obs", "burst", kind, "--count", &count]))
        };
        let changes = row![
            small_button("斷線", command(&["obs", "drop"])),
            text("突發").size(SMALL).color(MUTED),
            text_input("", &self.burst)
                .on_input(Message::BurstCount)
                .size(SMALL)
                .width(48),
            burst("切場景", "scenes"),
            burst("增減來源", "items"),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let stress = row![
            text("壓力場景來源數").size(SMALL).color(MUTED),
            text_input("", &self.stress)
                .on_input(Message::StressCount)
                .size(SMALL)
                .width(56),
            small_button("套用", command(&["obs", "sources", self.stress.trim()])),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let s = &o.stats;
        let stat = |label: String,
                    warn: bool,
                    bar: Element<'static, Message>|
         -> Element<'static, Message> {
            column![
                text(label)
                    .size(SMALL)
                    .color(if warn { RED } else { MUTED }),
                bar
            ]
            .spacing(1)
            .into()
        };
        let set =
            |flag: &'static str| move |v: f64| command(&["obs", "stats", flag, &format!("{v:.0}")]);
        let dropped = |v: f64| command(&["obs", "stats", "--dropped", &format!("{v:.1}")]);
        let meters = |extra: f64, every: f64| {
            command(&[
                "obs",
                "meters",
                "--inputs",
                &format!("{extra:.0}"),
                "--interval",
                &format!("{every:.0}"),
            ])
        };
        let (extra, every) = (o.meter_extra as f64, o.meter_interval.as_millis() as f64);
        let controls = column![
            toggles,
            changes,
            stress,
            stat(
                format!("CPU {:.0}%", s.cpu_percent),
                false,
                slider(0.0..=100.0, s.cpu_percent, set("--cpu")).into()
            ),
            stat(
                format!("掉幀 {:.1}%", s.dropped_percent),
                s.dropped_percent > 5.0,
                slider(0.0..=20.0, s.dropped_percent, dropped)
                    .step(0.1)
                    .into()
            ),
            stat(
                format!("位元率 {} kbps", s.bitrate_kbps),
                false,
                slider(0.0..=10_000.0, s.bitrate_kbps as f64, set("--kbps"))
                    .step(100.0)
                    .into()
            ),
            stat(
                format!("音量條額外來源 {extra:.0}"),
                false,
                slider(0.0..=50.0, extra, move |v| meters(v, every)).into()
            ),
            stat(
                format!("音量條間隔 {every:.0} ms"),
                false,
                slider(10.0..=1000.0, every, move |v| meters(extra, v))
                    .step(10.0)
                    .into()
            ),
            column(o.audio.iter().map(|a| {
                let db = if a.muted {
                    "靜音".to_owned()
                } else {
                    format!("{:.0} dB", a.volume_db)
                };
                row![
                    text(a.name.clone()).size(SMALL).width(90),
                    progress_bar(0.0..=1.0, a.level as f32).girth(8),
                    text(db).size(SMALL).width(52),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            }))
            .spacing(3),
        ]
        .spacing(8);

        row![
            column![
                heading("場景（按兩下切成節目）"),
                scrollable(scenes).height(Fill)
            ]
            .spacing(4)
            .width(170),
            column![scrollable(sources).height(Fill)].width(Fill),
            scrollable(controls).width(300).height(Fill),
        ]
        .spacing(12)
        .padding([10, 12])
        .into()
    }

    pub(super) fn faults_page(&self) -> Element<'_, Message> {
        let f = &self.fault_form;
        let input = |value: &str, on: fn(String) -> Message, width: f32| {
            text_input("", value).on_input(on).size(TEXT).width(width)
        };
        let form = row![
            labeled(
                "呼叫",
                text_input("GET /helix/users", &f.key)
                    .on_input(Message::FaultKey)
                    .size(TEXT)
                    .into()
            ),
            labeled(
                "狀態碼",
                input(&f.status, Message::FaultStatus, 60.0).into()
            ),
            labeled(
                "延遲 ms",
                input(&f.delay_ms, Message::FaultDelay, 70.0).into()
            ),
            labeled("持續秒", input(&f.for_s, Message::FaultFor, 60.0).into()),
            checkbox(f.close)
                .label("斷開")
                .text_size(SMALL)
                .on_toggle(Message::FaultClose),
            button(text("設定").size(TEXT))
                .padding([5, 14])
                .style(button::primary)
                .on_press(Message::Run(f.words())),
        ]
        .spacing(8)
        .align_y(Alignment::End);
        let suggestions = row(super::faults::suggestions(&self.report)
            .into_iter()
            .map(|key| {
                button(text(key.clone()).size(SMALL))
                    .padding([2, 6])
                    .style(button::text)
                    .on_press(Message::FaultKey(key))
                    .into()
            }))
        .spacing(4)
        .wrap();
        let active: Element<'_, Message> = if self.faults.is_empty() {
            text("沒有").color(MUTED).into()
        } else {
            type Row = (String, Fault, std::time::Duration);
            table::table(
                [
                    table::column(heading("呼叫"), |r: Row| text(r.0)).width(Fill),
                    table::column(heading("狀態碼"), |r: Row| {
                        text(r.1.status.map(|s| s.to_string()).unwrap_or_default())
                    }),
                    table::column(heading("延遲"), |r: Row| {
                        text(if r.1.delay.is_zero() {
                            String::new()
                        } else {
                            format!("{} ms", r.1.delay.as_millis())
                        })
                    }),
                    table::column(heading("斷開"), |r: Row| {
                        text(if r.1.close { "是" } else { "" })
                    }),
                    table::column(heading("剩餘"), |r: Row| {
                        text(format!("{} 秒", r.2.as_secs()))
                    }),
                    table::column(text(""), |r: Row| {
                        small_button("移除", Message::RemoveFault(r.0))
                    }),
                ],
                self.faults.clone(),
            )
            .padding_x(8)
            .padding_y(2)
            .into()
        };
        scrollable(
            column![
                heading("新增故障"),
                form,
                suggestions,
                text(format!("進行中（{}）", self.faults.len()))
                    .size(SMALL)
                    .color(MUTED),
                active,
            ]
            .spacing(8)
            .padding([10, 12]),
        )
        .height(Fill)
        .into()
    }
}

impl Panel {
    pub(super) fn stats_page(&self) -> Element<'_, Message> {
        use super::stats::{CALL_COLUMNS, cell, sorted_calls};
        let r = &self.report;
        let summary = row![
            text(format!(
                "歸零後 {} 秒",
                r["elapsed_s"].as_f64().unwrap_or(0.0) as u64
            )),
            text(format!(
                "Helix 每分鐘最多 {} 次（{}%）",
                cell(&r["helix"]["peak_per_minute"]),
                cell(&r["helix"]["bucket_percent"])
            )),
            text(format!("token 檢查 {} 次", cell(&r["validate"]["total"]))),
        ]
        .spacing(24);
        let warnings: Vec<Value> = r["warnings"].as_array().cloned().unwrap_or_default();
        let warnings: Element<'_, Message> = if warnings.is_empty() {
            text("沒有異常").color(MUTED).into()
        } else {
            column(warnings.iter().map(|w| {
                text(format!(
                    "{:.1} 秒  {}  {}",
                    w["at_s"].as_f64().unwrap_or(0.0),
                    cell(&w["key"]),
                    cell(&w["detail"])
                ))
                .size(SMALL)
                .color(AMBER)
                .into()
            }))
            .spacing(2)
            .into()
        };
        let sort = self.sort;
        let calls =
            table::table(
                CALL_COLUMNS.iter().enumerate().map(|(i, (field, _))| {
                    let head = button(text(sort.heading(i)).size(SMALL).color(MUTED))
                        .style(button::text)
                        .padding(0)
                        .on_press(Message::Sort(i));
                    table::column(head, move |row: Value| value_cell(&row[*field]))
                        .width(if i == 0 { Fill } else { Shrink })
                }),
                sorted_calls(r, sort),
            )
            .padding_x(8)
            .padding_y(2);
        const EVENTS: [(&str, &str); 5] = [
            ("kind", "事件"),
            ("sent", "送出"),
            ("calls_after", "之後的呼叫"),
            ("calls_per_event", "每事件"),
            ("avg_reaction_ms", "平均反應 ms"),
        ];
        const RETRIES: [(&str, &str); 4] = [
            ("key", "呼叫"),
            ("at_s", "秒"),
            ("retries", "重試"),
            ("gaps_ms", "間隔 ms"),
        ];
        let list = |key: &str| r[key].as_array().cloned().unwrap_or_default();
        scrollable(
            column![
                summary,
                heading("異常警告"),
                warnings,
                heading("呼叫（點欄位排序）"),
                calls,
                heading("事件之後"),
                report_table(&EVENTS, list("events")),
                heading("失敗後的重試"),
                report_table(&RETRIES, list("retries")),
            ]
            .spacing(8)
            .padding([10, 12]),
        )
        .height(Fill)
        .into()
    }
}

/// A report table as it is: one column per field, the first one wide; "沒有" when empty.
fn report_table<'a>(
    columns: &'static [(&'static str, &'static str)],
    rows: Vec<Value>,
) -> Element<'a, Message> {
    if rows.is_empty() {
        return text("沒有").color(MUTED).into();
    }
    table::table(
        columns.iter().enumerate().map(|(i, (field, label))| {
            table::column(heading(label), move |row: Value| value_cell(&row[*field]))
                .width(if i == 0 { Fill } else { Shrink })
        }),
        rows,
    )
    .padding_x(8)
    .padding_y(2)
    .into()
}

/// A report value; monospace only for plain ASCII (no Chinese glyphs there).
fn value_cell<'a>(value: &Value) -> Element<'a, Message> {
    let line = super::stats::cell(value);
    let cell = text(line.clone()).size(SMALL);
    if line.is_ascii() {
        cell.font(Font::MONOSPACE)
    } else {
        cell
    }
    .into()
}

/// A control command run in the panel, the same words as on the command line.
fn command(words: &[&str]) -> Message {
    Message::Run(words.iter().map(|w| (*w).to_owned()).collect())
}

fn labeled<'a>(label: &'static str, input: Element<'a, Message>) -> Element<'a, Message> {
    column![text(label).size(SMALL).color(MUTED), input]
        .spacing(2)
        .into()
}
