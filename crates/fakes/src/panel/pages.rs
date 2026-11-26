//! The tab pages.

use iced::{
    Alignment, Element,
    Length::{Fill, FillPortion},
    widget::{
        button, checkbox, column, container, pick_list, row, scrollable, table, text, text_input,
    },
};

use super::{
    AMBER, GREEN, MUTED, Message, Panel, SMALL, TEXT, heading, kv,
    send::{self, Field, Form, Kind, STATUSES, Status, TIERS},
    small_button,
};
use crate::twitch::{BROADCASTER_ID, BROADCASTER_LOGIN, REWARDS};

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
                    let titles: Vec<&'static str> = REWARDS.iter().map(|r| r.1).collect();
                    pick_list(titles, Some(form.reward), Message::Reward)
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
        let notice = self.notice.as_ref().map(|(line, warn)| {
            text(line.clone())
                .size(SMALL)
                .color(if *warn { AMBER } else { GREEN })
        });

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
            .push(notice)
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
                table::column(heading("獎勵"), |r: (&'static str, &'static str, u32)| {
                    text(r.1)
                })
                .width(Fill),
                table::column(heading("點數"), |r: (&'static str, &'static str, u32)| {
                    text(r.2.to_string())
                }),
                table::column(heading("ID"), |r: (&'static str, &'static str, u32)| {
                    text(r.0).size(SMALL).color(MUTED)
                }),
            ],
            REWARDS.iter().copied(),
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

fn labeled<'a>(label: &'static str, input: Element<'a, Message>) -> Element<'a, Message> {
    column![text(label).size(SMALL).color(MUTED), input]
        .spacing(2)
        .into()
}
