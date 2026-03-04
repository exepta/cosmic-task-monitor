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

        let sort_controls = widget::row::with_capacity(5)
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

        let list_rows = self.process_entries.iter().fold(
            widget::column::with_capacity(self.process_entries.len()),
            |column, process| {
                let name_cell_content: Element<'_, Message> =
                    if let Some(icon_handle) = process.icon_handle.as_ref() {
                        widget::row::with_capacity(2)
                            .push(icon::icon(icon_handle.clone()).size(18))
                            .push(widget::text(process.display_name.as_str()))
                            .align_y(Alignment::Center)
                            .spacing(space_s)
                            .into()
                    } else {
                        widget::text(process.display_name.as_str()).into()
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
                                widget::container(widget::text(process.threads.to_string()))
                                    .padding(10)
                                    .class(theme::Container::custom(table_cell_style))
                                    .width(Length::FillPortion(2)),
                            )
                            .spacing(0)
                            .width(Length::Fill),
                    )
                    .on_press(Message::OpenProcessMenu {
                        app_id: process.app_id.clone(),
                        display_name: process.display_name.clone(),
                        pid: process.pid,
                    })
                    .padding(0)
                    .class(table_row_button_style())
                    .width(Length::Fill),
                )
            },
        );

        match self.apps_view_mode {
            AppsViewMode::List => widget::column::with_capacity(3)
                .push(header)
                .push(sort_controls)
                .push(widget::scrollable(list_rows).height(Length::Fill))
                .spacing(space_s)
                .height(Length::Fill)
                .into(),
            AppsViewMode::Tile => {
                let tile_grid = widget::responsive(move |size| {
                    let spacing = space_s as f32;
                    let min_tile_width = 320.0;
                    let tile_columns = (((size.width + spacing) / (min_tile_width + spacing))
                        .floor() as usize)
                        .clamp(1, 4);

                    let mut tile_rows = widget::column::with_capacity(
                        (self.process_entries.len() + tile_columns - 1) / tile_columns,
                    )
                    .spacing(space_s)
                    .width(Length::Fill);

                    for chunk in self.process_entries.chunks(tile_columns) {
                        let mut tile_row = widget::row::with_capacity(tile_columns)
                            .spacing(space_s)
                            .width(Length::Fill);

                        for process in chunk {
                            let icon_content: Element<'_, Message> =
                                if let Some(icon_handle) = process.icon_handle.as_ref() {
                                    icon::icon(icon_handle.clone()).size(56).into()
                                } else {
                                    widget::container(widget::text(""))
                                        .width(Length::Fixed(56.0))
                                        .into()
                                };

                            let details = widget::column::with_capacity(5)
                                .push(widget::text(process.display_name.as_str()).size(20))
                                .push(
                                    widget::text(format!("{}: {}", fl!("table-pid"), process.pid))
                                        .size(12),
                                )
                                .push(
                                    widget::text(format!(
                                        "{}: {:.1}%",
                                        fl!("table-cpu"),
                                        process.cpu_percent
                                    ))
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
                                    widget::text(format!(
                                        "{}: {}",
                                        fl!("table-threads"),
                                        process.threads
                                    ))
                                    .size(12),
                                )
                                .spacing(6)
                                .width(Length::Fill);

                            let tile_content = widget::container(
                                widget::row::with_capacity(2)
                                    .push(
                                        widget::container(icon_content)
                                            .center_x(Length::Fixed(56.0)),
                                    )
                                    .push(details)
                                    .spacing(25)
                                    .align_y(Alignment::Center)
                                    .width(Length::Fill),
                            )
                            .padding(12)
                            .class(theme::Container::custom(table_cell_style))
                            .width(Length::Fill);

                            let tile_button = widget::button::custom(tile_content)
                                .on_press(Message::OpenProcessMenu {
                                    app_id: process.app_id.clone(),
                                    display_name: process.display_name.clone(),
                                    pid: process.pid,
                                })
                                .padding(0)
                                .class(table_row_button_style())
                                .width(Length::Fill);

                            tile_row = tile_row
                                .push(widget::container(tile_button).width(Length::FillPortion(1)));
                        }

                        for _ in chunk.len()..tile_columns {
                            tile_row = tile_row.push(
                                widget::container(widget::text(""))
                                    .width(Length::FillPortion(1))
                                    .height(Length::Shrink),
                            );
                        }

                        tile_rows = tile_rows.push(tile_row);
                    }

                    widget::scrollable(tile_rows).height(Length::Fill).into()
                });

                widget::column::with_capacity(3)
                    .push(header)
                    .push(sort_controls)
                    .push(tile_grid)
                    .spacing(space_s)
                    .height(Length::Fill)
                    .into()
            }
        }
    }
}
