// SPDX-License-Identifier: MPL-2.0

use super::*;

impl AppModel {
    pub(super) fn apps_view(&self, space_s: u16) -> Element<'_, Message> {
        let header = widget::row::with_capacity(1)
            .push(widget::text::title2(fl!(
                "apps-title",
                count = self.process_entries.len()
            )))
            .align_y(Alignment::Center)
            .spacing(space_s);

        let desktop_entries = self
            .process_entries
            .iter()
            .filter(|entry| !entry.is_background)
            .cloned()
            .collect::<Vec<_>>();
        let background_entries = self
            .process_entries
            .iter()
            .filter(|entry| entry.is_background)
            .cloned()
            .collect::<Vec<_>>();

        let content = widget::column::with_capacity(3)
            .push(header)
            .push(self.apps_section(
                fl!("autostart-desktop-apps"),
                self.apps_desktop_expanded,
                Message::ToggleAppsDesktopSection,
                &desktop_entries,
                space_s,
            ))
            .push(self.apps_section(
                fl!("autostart-background-apps"),
                self.apps_background_expanded,
                Message::ToggleAppsBackgroundSection,
                &background_entries,
                space_s,
            ))
            .spacing(space_s)
            .width(Length::Fill);

        widget::container(widget::scrollable(content).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn apps_section(
        &self,
        title: String,
        expanded: bool,
        toggle_message: Message,
        entries: &[ProcessEntry],
        space_s: u16,
    ) -> Element<'_, Message> {
        let arrow_icon_name = if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        };
        let section_title = format!("{title} ({})", entries.len());

        let header_button = widget::button::custom(
            widget::row::with_capacity(3)
                .push(widget::text(section_title).size(14))
                .push(
                    widget::icon::from_name(arrow_icon_name)
                        .icon()
                        .size(16)
                        .class(theme::Svg::custom(|theme| {
                            cosmic::iced_widget::svg::Style {
                                color: Some(theme.cosmic().accent_color().into()),
                            }
                        })),
                )
                .push(widget::horizontal_space())
                .spacing(8)
                .width(Length::Fill)
                .align_y(Alignment::Center),
        )
        .on_press(toggle_message)
        .padding(0)
        .class(section_toggle_button_style())
        .width(Length::Fill);

        let mut section = widget::column::with_capacity(2)
            .push(
                widget::container(header_button)
                    .padding(10)
                    .width(Length::Fill),
            )
            .spacing(space_s);

        if expanded {
            section = section.push(match self.apps_view_mode {
                AppsViewMode::List => self.apps_table(entries, space_s),
                AppsViewMode::Tile => self.apps_tiles(entries, space_s),
            });
        }

        section.into()
    }

    fn apps_table(&self, entries: &[ProcessEntry], space_s: u16) -> Element<'_, Message> {
        let owned_entries = entries.to_vec();
        let entry_count = owned_entries.len();

        let list_headers = widget::row::with_capacity(5)
            .push(
                widget::container(
                    widget::button::custom(
                        self.header_button_content(fl!("table-name"), SortColumn::Name),
                    )
                    .on_press(Message::ToggleSort(SortColumn::Name))
                    .width(Length::Fill),
                )
                .padding(10)
                .class(theme::Container::custom(table_cell_style))
                .width(Length::FillPortion(6)),
            )
            .push(
                widget::container(
                    widget::button::custom(
                        self.header_button_content(fl!("table-cpu"), SortColumn::Cpu),
                    )
                    .on_press(Message::ToggleSort(SortColumn::Cpu))
                    .width(Length::Fill),
                )
                .padding(10)
                .class(theme::Container::custom(table_cell_style))
                .width(Length::FillPortion(2)),
            )
            .push(
                widget::container(
                    widget::button::custom(
                        self.header_button_content(fl!("table-pid"), SortColumn::Pid),
                    )
                    .on_press(Message::ToggleSort(SortColumn::Pid))
                    .width(Length::Fill),
                )
                .padding(10)
                .class(theme::Container::custom(table_cell_style))
                .width(Length::FillPortion(2)),
            )
            .push(
                widget::container(
                    widget::button::custom(
                        self.header_button_content(fl!("table-ram"), SortColumn::Ram),
                    )
                    .on_press(Message::ToggleSort(SortColumn::Ram))
                    .width(Length::Fill),
                )
                .padding(10)
                .class(theme::Container::custom(table_cell_style))
                .width(Length::FillPortion(2)),
            )
            .push(
                widget::container(
                    widget::button::custom(
                        self.header_button_content(fl!("table-threads"), SortColumn::Threads),
                    )
                    .on_press(Message::ToggleSort(SortColumn::Threads))
                    .width(Length::Fill),
                )
                .padding(10)
                .class(theme::Container::custom(table_cell_style))
                .width(Length::FillPortion(2)),
            )
            .spacing(0);

        let rows: Element<'_, Message> = if owned_entries.is_empty() {
            widget::container(widget::text(fl!("autostart-section-empty")))
                .padding(10)
                .class(theme::Container::custom(table_cell_style))
                .width(Length::Fill)
                .into()
        } else {
            owned_entries
                .into_iter()
                .fold(
                    widget::column::with_capacity(entry_count),
                    |column, process| {
                        let name_cell_content: Element<'_, Message> =
                            if let Some(icon_handle) = process.icon_handle.as_ref() {
                                widget::row::with_capacity(2)
                                .push(icon::icon(icon_handle.clone()).size(18))
                                .push(
                                    widget::text(process.display_name.clone())
                                        .width(Length::Fill)
                                        .wrapping(cosmic::iced::widget::text::Wrapping::None)
                                        .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                            cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                                        )),
                                )
                                .align_y(Alignment::Center)
                                .spacing(space_s)
                                .width(Length::Fill)
                                .into()
                            } else {
                                widget::text(process.display_name.clone())
                                    .width(Length::Fill)
                                    .wrapping(cosmic::iced::widget::text::Wrapping::None)
                                    .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                        cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                                    ))
                                    .into()
                            };

                        column.push(
                            widget::button::custom(
                                widget::row::with_capacity(5)
                                    .push(
                                        widget::container(name_cell_content)
                                            .padding(10)
                                            .class(theme::Container::custom(table_cell_style))
                                            .width(Length::FillPortion(6)),
                                    )
                                    .push(
                                        widget::container(widget::text(format!(
                                            "{:.1}%",
                                            process.cpu_percent
                                        )))
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(2)),
                                    )
                                    .push(
                                        widget::container(widget::text(process.pid.to_string()))
                                            .padding(10)
                                            .class(theme::Container::custom(table_cell_style))
                                            .width(Length::FillPortion(2)),
                                    )
                                    .push(
                                        widget::container(widget::text(Self::format_rss(
                                            process.rss_bytes,
                                        )))
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(2)),
                                    )
                                    .push(
                                        widget::container(widget::text(
                                            process.threads.to_string(),
                                        ))
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(2)),
                                    )
                                    .spacing(0)
                                    .width(Length::Fill),
                            )
                            .on_press(Message::OpenProcessMenu {
                                app_id: process.app_id,
                                display_name: process.display_name,
                                pid: process.pid,
                            })
                            .padding(0)
                            .class(table_row_button_style())
                            .width(Length::Fill),
                        )
                    },
                )
                .into()
        };

        widget::column::with_capacity(2)
            .push(list_headers)
            .push(rows)
            .spacing(0)
            .width(Length::Fill)
            .into()
    }

    fn apps_tiles(&self, entries: &[ProcessEntry], space_s: u16) -> Element<'_, Message> {
        let owned_entries = entries.to_vec();
        if owned_entries.is_empty() {
            return widget::container(widget::text(fl!("autostart-section-empty")))
                .padding(10)
                .class(theme::Container::custom(table_cell_style))
                .width(Length::Fill)
                .into();
        }

        let tiles: Vec<Element<'_, Message>> = owned_entries
            .into_iter()
            .map(|process| {
                let icon_content: Element<'_, Message> =
                    if let Some(icon_handle) = process.icon_handle.as_ref() {
                        icon::icon(icon_handle.clone()).size(56).into()
                    } else {
                        widget::container(widget::text(""))
                            .width(Length::Fixed(56.0))
                            .into()
                    };

                let tile_name = process.display_name.clone();
                let tile_app_id = process.app_id.clone();
                let tile_pid = process.pid;

                let details = widget::column::with_capacity(5)
                    .push(
                        widget::text(tile_name.clone())
                            .size(20)
                            .width(Length::Fill)
                            .wrapping(cosmic::iced::widget::text::Wrapping::None)
                            .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                            )),
                    )
                    .push(widget::text(format!("{}: {}", fl!("table-pid"), tile_pid)).size(12))
                    .push(
                        widget::text(format!("{}: {:.1}%", fl!("table-cpu"), process.cpu_percent))
                            .size(12),
                    )
                    .push(
                        widget::text(format!(
                            "{}: {}",
                            fl!("table-ram"),
                            Self::format_rss(process.rss_bytes)
                        ))
                        .size(12),
                    )
                    .push(
                        widget::text(format!("{}: {}", fl!("table-threads"), process.threads))
                            .size(12),
                    )
                    .spacing(6)
                    .width(Length::Fill);

                let tile_content = widget::container(
                    widget::row::with_capacity(2)
                        .push(widget::container(icon_content).center_x(Length::Fixed(56.0)))
                        .push(details)
                        .spacing(25)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                )
                .padding(12)
                .class(theme::Container::custom(table_cell_style))
                .width(Length::Fill);

                widget::container(
                    widget::button::custom(tile_content)
                        .on_press(Message::OpenProcessMenu {
                            app_id: tile_app_id,
                            display_name: tile_name,
                            pid: tile_pid,
                        })
                        .padding(0)
                        .class(table_row_button_style())
                        .width(Length::Fill),
                )
                .width(Length::Fill)
                .into()
            })
            .collect();

        widget::container(
            widget::flex_row(tiles)
                .spacing(space_s)
                .min_item_width(320.0)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .into()
    }
}
