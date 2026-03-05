// SPDX-License-Identifier: MPL-2.0

use super::*;
use std::ffi::OsStr;

#[derive(Default)]
struct ParsedDesktopEntry {
    name: Option<String>,
    exec: Option<String>,
    icon: Option<String>,
    hidden: bool,
    no_display: bool,
    autostart_enabled: bool,
    only_show_in: Vec<String>,
    not_show_in: Vec<String>,
}

impl AppModel {
    fn set_autostart_feedback(
        &mut self,
        level: AutostartFeedbackLevel,
        message: String,
        expires_at: Option<Instant>,
    ) {
        self.autostart_feedback = Some(AutostartFeedback {
            level,
            message,
            expires_at,
        });
    }

    pub(super) fn dismiss_autostart_feedback(&mut self) {
        self.autostart_feedback = None;
    }

    pub(super) fn clear_expired_autostart_feedback(&mut self) {
        let should_clear = self
            .autostart_feedback
            .as_ref()
            .and_then(|feedback| feedback.expires_at)
            .is_some_and(|expires_at| Instant::now() >= expires_at);
        if should_clear {
            self.autostart_feedback = None;
        }
    }

    pub(super) fn refresh_autostart_state(&mut self) {
        self.autostart_entries = Self::load_autostart_entries(&self.desktop_apps_by_exec);
        self.autostart_add_options =
            Self::build_autostart_add_options(&self.desktop_apps_by_exec, &self.autostart_entries);

        self.autostart_modal_selected_option = match self.autostart_modal_selected_option {
            Some(index) if index < self.autostart_add_options.len() => Some(index),
            _ if !self.autostart_add_options.is_empty() => Some(0),
            _ => None,
        };
    }

    pub(super) fn open_autostart_modal(&mut self) {
        self.autostart_modal_open = true;
        if self.autostart_modal_selected_option.is_none() && !self.autostart_add_options.is_empty()
        {
            self.autostart_modal_selected_option = Some(0);
        }
    }

    pub(super) fn open_selected_autostart_path(&mut self) {
        let Some(selected) = self.selected_autostart_entry.as_ref() else {
            return;
        };
        let path = PathBuf::from(&selected.autostart_path);
        let open_path = path
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or(path);
        if let Err(err) = open::that_detached(open_path) {
            eprintln!("failed to open autostart path: {err}");
        }
    }

    pub(super) fn edit_selected_autostart_desktop(&mut self) {
        let Some(selected) = self.selected_autostart_entry.as_ref() else {
            return;
        };
        if let Err(err) = open::that_detached(PathBuf::from(&selected.autostart_path)) {
            eprintln!("failed to open autostart desktop file: {err}");
        }
    }

    pub(super) fn request_remove_selected_autostart(&mut self) {
        if self.selected_autostart_entry.is_none() {
            return;
        }
        self.autostart_remove_modal_open = true;
        self.core.window.show_context = false;
    }

    pub(super) fn cancel_remove_selected_autostart(&mut self) {
        self.autostart_remove_modal_open = false;
    }

    pub(super) fn confirm_remove_selected_autostart(&mut self) {
        let Some(selected) = self.selected_autostart_entry.as_ref().cloned() else {
            self.autostart_remove_modal_open = false;
            return;
        };

        match Self::remove_autostart_entry(&selected.autostart_path, selected.is_background) {
            Ok(()) => {
                self.refresh_autostart_state();
                self.set_autostart_feedback(
                    AutostartFeedbackLevel::Success,
                    fl!("autostart-feedback-remove-success", name = selected.name),
                    Some(Instant::now() + AUTOSTART_FEEDBACK_TIMEOUT),
                );
                self.autostart_remove_modal_open = false;
                self.core.window.show_context = false;
                self.selected_autostart_entry = None;
            }
            Err(err) => {
                self.set_autostart_feedback(
                    AutostartFeedbackLevel::Error,
                    fl!(
                        "autostart-feedback-remove-failed",
                        name = selected.name,
                        error = err.to_string()
                    ),
                    None,
                );
                self.autostart_remove_modal_open = false;
            }
        }
    }

    pub(super) fn create_custom_autostart_desktop(&mut self) {
        match Self::write_custom_autostart_entry_template() {
            Ok(path) => {
                if let Err(err) = open::that_detached(path.clone()) {
                    self.set_autostart_feedback(
                        AutostartFeedbackLevel::Error,
                        fl!("autostart-feedback-custom-failed", error = err.to_string()),
                        None,
                    );
                    return;
                }

                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                self.refresh_autostart_state();
                self.autostart_modal_open = false;
                self.set_autostart_feedback(
                    AutostartFeedbackLevel::Success,
                    fl!("autostart-feedback-custom-created", file = file_name),
                    Some(Instant::now() + AUTOSTART_FEEDBACK_TIMEOUT),
                );
                self.autostart_desktop_expanded = true;
                self.autostart_background_expanded = true;
            }
            Err(err) => {
                self.set_autostart_feedback(
                    AutostartFeedbackLevel::Error,
                    fl!("autostart-feedback-custom-failed", error = err.to_string()),
                    None,
                );
            }
        }
    }

    pub(super) fn import_autostart_desktop_from_file(&mut self) {
        let picked = match Self::pick_desktop_file_path() {
            Ok(path) => path,
            Err(err) => {
                self.set_autostart_feedback(
                    AutostartFeedbackLevel::Error,
                    fl!("autostart-feedback-import-failed", error = err.to_string()),
                    None,
                );
                return;
            }
        };

        let Some(source_path) = picked else {
            return;
        };

        match Self::import_desktop_file_to_autostart(&source_path) {
            Ok(target) => {
                let file_name = target
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| target.to_string_lossy().to_string());
                self.refresh_autostart_state();
                self.autostart_modal_open = false;
                self.set_autostart_feedback(
                    AutostartFeedbackLevel::Success,
                    fl!("autostart-feedback-import-success", file = file_name),
                    Some(Instant::now() + AUTOSTART_FEEDBACK_TIMEOUT),
                );
                self.autostart_desktop_expanded = true;
                self.autostart_background_expanded = true;
            }
            Err(err) => {
                self.set_autostart_feedback(
                    AutostartFeedbackLevel::Error,
                    fl!("autostart-feedback-import-failed", error = err.to_string()),
                    None,
                );
            }
        }
    }

    pub(super) fn confirm_autostart_modal(&mut self) {
        let Some(index) = self.autostart_modal_selected_option else {
            self.set_autostart_feedback(
                AutostartFeedbackLevel::Error,
                fl!("autostart-feedback-no-selection"),
                None,
            );
            return;
        };
        let Some(option) = self.autostart_add_options.get(index).cloned() else {
            self.set_autostart_feedback(
                AutostartFeedbackLevel::Error,
                fl!("autostart-feedback-no-selection"),
                None,
            );
            return;
        };
        if let Err(err) = Self::write_autostart_entry(&option) {
            eprintln!("failed to add autostart entry {}: {err}", option.name);
            self.set_autostart_feedback(
                AutostartFeedbackLevel::Error,
                fl!(
                    "autostart-feedback-create-failed",
                    name = option.name,
                    error = err.to_string()
                ),
                None,
            );
            return;
        }

        self.refresh_autostart_state();
        self.set_autostart_feedback(
            AutostartFeedbackLevel::Success,
            fl!("autostart-feedback-create-success", name = option.name),
            Some(Instant::now() + AUTOSTART_FEEDBACK_TIMEOUT),
        );
        self.autostart_modal_open = false;
        self.autostart_desktop_expanded = true;
        self.autostart_background_expanded = true;
    }

    pub(super) fn autostart_add_dialog(&self) -> Option<Element<'_, Message>> {
        if !self.autostart_modal_open {
            return None;
        }

        let options_content: Element<'_, Message> = if self.autostart_add_options.is_empty() {
            widget::container(widget::text(fl!("autostart-add-none")))
                .padding(10)
                .width(Length::Fill)
                .into()
        } else {
            let list = self.autostart_add_options.iter().enumerate().fold(
                widget::column::with_capacity(self.autostart_add_options.len()),
                |column, (index, option)| {
                    let selected = self.autostart_modal_selected_option == Some(index);
                    let marker = if selected { "●" } else { "○" };
                    let row = widget::row::with_capacity(3)
                        .push(widget::text(marker))
                        .push(
                            widget::text(option.name.clone())
                                .width(Length::FillPortion(3))
                                .wrapping(cosmic::iced::widget::text::Wrapping::None)
                                .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                    cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                                )),
                        )
                        .push(
                            widget::text(option.app_id.clone())
                                .width(Length::FillPortion(2))
                                .wrapping(cosmic::iced::widget::text::Wrapping::None)
                                .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                    cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                                )),
                        )
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .width(Length::Fill);

                    column.push(
                        widget::button::custom(
                            widget::container(row)
                                .padding(8)
                                .class(theme::Container::custom(table_cell_style))
                                .width(Length::Fill),
                        )
                        .on_press(Message::SelectAutostartModalOption(index))
                        .padding(0)
                        .class(table_row_button_style())
                        .width(Length::Fill),
                    )
                },
            );
            widget::container(widget::scrollable(list).height(Length::Fixed(320.0)))
                .width(Length::Fill)
                .into()
        };
        let custom_controls: Element<'_, Message> = widget::container(
            widget::column::with_capacity(4)
                .push(widget::text(fl!("autostart-custom-description")).size(13))
                .push(
                    widget::button::standard(fl!("autostart-custom-button"))
                        .on_press(Message::CreateCustomAutostartDesktop),
                )
                .push(widget::text(fl!("autostart-import-description")).size(13))
                .push(
                    widget::button::standard(fl!("autostart-import-button"))
                        .on_press(Message::ImportAutostartDesktopFromFile),
                )
                .spacing(8)
                .width(Length::Fill),
        )
        .padding([0, 0, 8, 0])
        .width(Length::Fill)
        .into();

        let control_content: Element<'_, Message> = if let Some(feedback) = self
            .autostart_feedback
            .as_ref()
            .filter(|feedback| feedback.level == AutostartFeedbackLevel::Error)
        {
            widget::column::with_capacity(3)
                .push(custom_controls)
                .push(widget::text(feedback.message.clone()).size(14))
                .push(options_content)
                .spacing(8)
                .width(Length::Fill)
                .into()
        } else {
            widget::column::with_capacity(2)
                .push(custom_controls)
                .push(options_content)
                .spacing(8)
                .width(Length::Fill)
                .into()
        };

        let mut create_button = widget::button::standard(fl!("autostart-modal-create"));
        if !self.autostart_add_options.is_empty() && self.autostart_modal_selected_option.is_some()
        {
            create_button = create_button.on_press(Message::ConfirmAutostartModal);
        }

        Some(
            widget::dialog()
                .title(fl!("autostart-modal-title"))
                .body(fl!("autostart-modal-description"))
                .control(control_content)
                .secondary_action(
                    widget::button::standard(fl!("autostart-modal-cancel"))
                        .on_press(Message::CloseAutostartModal),
                )
                .primary_action(create_button)
                .max_width(720.0)
                .into(),
        )
    }

    pub(super) fn autostart_remove_dialog(&self) -> Option<Element<'_, Message>> {
        if !self.autostart_remove_modal_open {
            return None;
        }
        let Some(selected) = self.selected_autostart_entry.as_ref() else {
            return None;
        };

        let description = if selected.is_background {
            fl!(
                "autostart-remove-modal-description-background",
                name = selected.name.clone()
            )
        } else {
            fl!(
                "autostart-remove-modal-description",
                name = selected.name.clone()
            )
        };

        Some(
            widget::dialog()
                .title(fl!("autostart-remove-modal-title"))
                .body(description)
                .control(widget::container(
                    widget::text(selected.autostart_path.clone()).size(12),
                ))
                .secondary_action(
                    widget::button::standard(fl!("autostart-modal-cancel"))
                        .on_press(Message::CancelRemoveSelectedAutostart),
                )
                .primary_action(
                    widget::button::destructive(fl!("autostart-action-remove"))
                        .on_press(Message::ConfirmRemoveSelectedAutostart),
                )
                .max_width(720.0)
                .into(),
        )
    }

    fn autostart_view_header(&self, space_s: u16) -> widget::Row<'_, Message> {
        widget::row::with_capacity(1)
            .push(widget::text::title2(fl!(
                "autostart-title",
                count = self.autostart_entries.len()
            )))
            .align_y(Alignment::Center)
            .spacing(space_s)
    }

    pub(super) fn autostart_view(&self, space_s: u16) -> Element<'_, Message> {
        let header = self.autostart_view_header(space_s);
        let desktop_entries = self
            .autostart_entries
            .iter()
            .filter(|entry| !entry.is_background)
            .cloned()
            .collect::<Vec<_>>();
        let background_entries = self
            .autostart_entries
            .iter()
            .filter(|entry| entry.is_background)
            .cloned()
            .collect::<Vec<_>>();
        let add_controls: Element<'_, Message> = widget::row::with_capacity(1)
            .push(
                widget::button::standard(fl!("autostart-add-button"))
                    .width(Length::Shrink)
                    .height(Length::Fixed(38.0))
                    .on_press(Message::OpenAutostartModal),
            )
            .align_y(Alignment::Center)
            .into();
        let feedback_banner = self
            .autostart_feedback
            .as_ref()
            .filter(|feedback| feedback.level == AutostartFeedbackLevel::Success)
            .map(|feedback| {
                let green = Color::from_rgb(39.0 / 255.0, 155.0 / 255.0, 77.0 / 255.0);
                let dismiss_button = widget::button::custom(widget::text("x").size(16))
                    .on_press(Message::DismissAutostartFeedback)
                    .padding([0, 8])
                    .class(theme::Button::Text);

                widget::container(
                    widget::row::with_capacity(2)
                        .push(
                            widget::text(feedback.message.clone())
                                .size(14)
                                .width(Length::Fill),
                        )
                        .push(dismiss_button)
                        .align_y(Alignment::Center)
                        .spacing(8)
                        .width(Length::Fill),
                )
                .padding([10, 12])
                .class(theme::Container::custom(move |_theme| {
                    widget::container::Style {
                        background: Some(Background::Color(Color { a: 0.14, ..green })),
                        border: Border {
                            color: green,
                            width: 1.0,
                            radius: 10.0.into(),
                        },
                        ..Default::default()
                    }
                }))
                .width(Length::Fill)
            });

        let mut content = widget::column::with_capacity(5)
            .push(header)
            .push(add_controls);
        if let Some(feedback_banner) = feedback_banner {
            content = content.push(feedback_banner);
        }
        content = content
            .push(self.autostart_section_table(
                fl!("autostart-desktop-apps"),
                self.autostart_desktop_expanded,
                Message::ToggleAutostartDesktopSection,
                &desktop_entries,
                space_s,
            ))
            .push(self.autostart_section_table(
                fl!("autostart-background-apps"),
                self.autostart_background_expanded,
                Message::ToggleAutostartBackgroundSection,
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

    fn autostart_section_table(
        &self,
        title: String,
        expanded: bool,
        toggle_message: Message,
        entries: &[AutostartEntry],
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
                AppsViewMode::List => self.autostart_table(entries, space_s),
                AppsViewMode::Tile => self.autostart_tiles(entries, space_s),
            });
        }

        section.into()
    }

    fn autostart_table(&self, entries: &[AutostartEntry], space_s: u16) -> Element<'_, Message> {
        let entry_count = entries.len();
        let owned_entries = entries.to_vec();

        let list_headers = widget::row::with_capacity(3)
            .push(
                widget::container(widget::text(fl!("table-name")))
                    .padding(10)
                    .class(theme::Container::custom(table_cell_style))
                    .width(Length::FillPortion(4)),
            )
            .push(
                widget::container(widget::text(fl!("autostart-table-path")))
                    .padding(10)
                    .class(theme::Container::custom(table_cell_style))
                    .width(Length::FillPortion(3)),
            )
            .push(
                widget::container(widget::text(fl!("autostart-table-exec")))
                    .padding(10)
                    .class(theme::Container::custom(table_cell_style))
                    .width(Length::FillPortion(5)),
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
                    |column, entry| {
                        let menu_name = entry.name.clone();
                        let menu_autostart_path = entry.autostart_path.clone();
                        let menu_is_background = entry.is_background;
                        let display_name = entry.name.clone();
                        let display_path = entry.autostart_path.clone();
                        let display_exec = entry.exec.clone();
                        let name_cell: Element<'_, Message> =
                            if let Some(icon_handle) = entry.icon_handle {
                                widget::row::with_capacity(2)
                                    .push(icon::icon(icon_handle).size(18))
                                    .push(
                                        widget::text(display_name.clone())
                                            .width(Length::Fill)
                                            .wrapping(cosmic::iced::widget::text::Wrapping::None)
                                            .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                                cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                                            )),
                                    )
                                    .align_y(Alignment::Center)
                                    .width(Length::Fill)
                                    .spacing(space_s)
                                    .into()
                            } else {
                                widget::text(display_name.clone())
                                    .width(Length::Fill)
                                    .wrapping(cosmic::iced::widget::text::Wrapping::None)
                                    .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                        cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                                    ))
                                    .into()
                            };

                        column.push(
                            widget::button::custom(
                                widget::row::with_capacity(3)
                                    .push(
                                        widget::container(name_cell)
                                            .padding(10)
                                            .class(theme::Container::custom(table_cell_style))
                                            .width(Length::FillPortion(4)),
                                    )
                                    .push(
                                        widget::container(
                                            widget::text(display_path.clone())
                                                .width(Length::Fill)
                                                .wrapping(cosmic::iced::widget::text::Wrapping::None)
                                                .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                                    cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                                                )),
                                        )
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(3)),
                                    )
                                    .push(
                                        widget::container(
                                            widget::text(display_exec)
                                                .width(Length::Fill)
                                                .wrapping(cosmic::iced::widget::text::Wrapping::None)
                                                .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                                    cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                                                )),
                                        )
                                        .padding(10)
                                        .class(theme::Container::custom(table_cell_style))
                                        .width(Length::FillPortion(5)),
                                    )
                                    .spacing(0)
                                    .width(Length::Fill),
                            )
                            .on_press(Message::OpenAutostartEntryMenu {
                                name: menu_name,
                                autostart_path: menu_autostart_path,
                                is_background: menu_is_background,
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

    fn autostart_tiles(&self, entries: &[AutostartEntry], space_s: u16) -> Element<'_, Message> {
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
            .map(|entry| {
                let menu_name = entry.name.clone();
                let menu_autostart_path = entry.autostart_path.clone();
                let menu_is_background = entry.is_background;
                let display_name = entry.name.clone();
                let display_path = entry.autostart_path.clone();
                let display_exec = entry.exec.clone();
                let icon_content: Element<'_, Message> =
                    if let Some(icon_handle) = entry.icon_handle {
                        icon::icon(icon_handle).size(56).into()
                    } else {
                        widget::container(widget::text(""))
                            .width(Length::Fixed(56.0))
                            .into()
                    };

                let details = widget::column::with_capacity(3)
                    .push(
                        widget::text(display_name)
                            .size(20)
                            .width(Length::Fill)
                            .wrapping(cosmic::iced::widget::text::Wrapping::None)
                            .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                            )),
                    )
                    .push(
                        widget::text(display_path)
                            .size(12)
                            .width(Length::Fill)
                            .wrapping(cosmic::iced::widget::text::Wrapping::None)
                            .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                            )),
                    )
                    .push(
                        widget::text(display_exec)
                            .size(12)
                            .width(Length::Fill)
                            .wrapping(cosmic::iced::widget::text::Wrapping::None)
                            .ellipsize(cosmic::iced::widget::text::Ellipsize::End(
                                cosmic::iced_core::text::EllipsizeHeightLimit::Lines(1),
                            )),
                    )
                    .spacing(6)
                    .width(Length::Fill);

                widget::container(
                    widget::button::custom(
                        widget::container(
                            widget::row::with_capacity(2)
                                .push(widget::container(icon_content).center_x(Length::Fixed(56.0)))
                                .push(details)
                                .spacing(25)
                                .align_y(Alignment::Center)
                                .width(Length::Fill),
                        )
                        .padding(12)
                        .class(theme::Container::custom(table_cell_style))
                        .width(Length::Fill),
                    )
                    .on_press(Message::OpenAutostartEntryMenu {
                        name: menu_name,
                        autostart_path: menu_autostart_path,
                        is_background: menu_is_background,
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

    fn load_autostart_entries(
        desktop_apps_by_exec: &HashMap<String, DesktopAppMeta>,
    ) -> Vec<AutostartEntry> {
        let current_desktops = Self::current_desktops();
        let desktop_metas = Self::unique_desktop_metas(desktop_apps_by_exec);
        let mut entries = Vec::new();
        let mut seen_file_names = HashSet::new();

        for dir in Self::autostart_dirs() {
            let Ok(read_dir) = fs::read_dir(&dir) else {
                continue;
            };
            let mut files = read_dir
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|ext| ext == OsStr::new("desktop"))
                })
                .collect::<Vec<_>>();
            files.sort();

            for path in files {
                let Some(file_name) = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToString::to_string)
                else {
                    continue;
                };
                if !seen_file_names.insert(file_name.clone()) {
                    continue;
                }

                let Some(parsed) = Self::parse_desktop_entry(&path) else {
                    continue;
                };
                if parsed.hidden || !parsed.autostart_enabled {
                    continue;
                }
                if !Self::desktop_entry_matches_current(&parsed, &current_desktops) {
                    continue;
                }

                let stem = file_name
                    .strip_suffix(".desktop")
                    .unwrap_or(file_name.as_str());
                let matched_meta = desktop_metas
                    .iter()
                    .find(|meta| {
                        meta.desktop_entry_id
                            .as_deref()
                            .is_some_and(|id| id == file_name || id == stem)
                            || meta.app_id == stem
                    })
                    .cloned();

                let app_id = matched_meta
                    .as_ref()
                    .map(|meta| meta.app_id.clone())
                    .unwrap_or_else(|| stem.to_string());
                let name = parsed
                    .name
                    .clone()
                    .or_else(|| matched_meta.as_ref().map(|meta| meta.name.clone()))
                    .unwrap_or_else(|| stem.to_string());
                let exec = parsed
                    .exec
                    .clone()
                    .or_else(|| {
                        matched_meta
                            .as_ref()
                            .and_then(|meta| meta.exec_command.clone())
                    })
                    .unwrap_or_else(|| fl!("gpu-not-available"));

                entries.push(AutostartEntry {
                    app_id,
                    desktop_file_name: file_name,
                    autostart_path: path.to_string_lossy().to_string(),
                    name,
                    exec,
                    is_background: parsed.no_display,
                    icon_handle: matched_meta.and_then(|meta| meta.icon_handle),
                });
            }
        }

        entries.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
                .then_with(|| a.desktop_file_name.cmp(&b.desktop_file_name))
        });
        entries
    }

    fn build_autostart_add_options(
        desktop_apps_by_exec: &HashMap<String, DesktopAppMeta>,
        autostart_entries: &[AutostartEntry],
    ) -> Vec<AutostartAddOption> {
        let active_ids = autostart_entries
            .iter()
            .flat_map(|entry| {
                let stem = entry
                    .desktop_file_name
                    .strip_suffix(".desktop")
                    .unwrap_or(entry.desktop_file_name.as_str())
                    .to_string();
                [
                    entry.desktop_file_name.to_ascii_lowercase(),
                    stem.to_ascii_lowercase(),
                    entry.app_id.to_ascii_lowercase(),
                ]
            })
            .collect::<HashSet<_>>();

        let mut options = Self::unique_desktop_metas(desktop_apps_by_exec)
            .into_iter()
            .filter(|meta| {
                let stem = meta
                    .desktop_entry_id
                    .as_deref()
                    .and_then(|id| id.strip_suffix(".desktop").map(ToString::to_string))
                    .unwrap_or_else(|| meta.app_id.clone())
                    .to_ascii_lowercase();
                let entry_id = meta
                    .desktop_entry_id
                    .clone()
                    .unwrap_or_else(|| format!("{stem}.desktop"))
                    .to_ascii_lowercase();
                !active_ids.contains(&meta.app_id.to_ascii_lowercase())
                    && !active_ids.contains(&entry_id)
                    && !active_ids.contains(&stem)
            })
            .map(|meta| AutostartAddOption {
                app_id: meta.app_id,
                desktop_entry_id: meta.desktop_entry_id,
                name: meta.name,
                exec: meta.exec_command,
                desktop_entry_path: meta.desktop_entry_path,
            })
            .collect::<Vec<_>>();

        options.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
                .then_with(|| a.app_id.cmp(&b.app_id))
        });
        options
    }

    fn unique_desktop_metas(
        desktop_apps_by_exec: &HashMap<String, DesktopAppMeta>,
    ) -> Vec<DesktopAppMeta> {
        let mut unique = HashMap::new();
        for meta in desktop_apps_by_exec.values() {
            unique
                .entry(meta.app_id.clone())
                .or_insert_with(|| meta.clone());
        }
        unique.into_values().collect()
    }

    fn write_autostart_entry(option: &AutostartAddOption) -> std::io::Result<()> {
        let autostart_dir = Self::user_autostart_dir();
        fs::create_dir_all(&autostart_dir)?;

        let file_name_raw = option
            .desktop_entry_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| option.app_id.clone());
        let file_name = Self::normalize_desktop_file_name(&file_name_raw);
        let target = autostart_dir.join(file_name);

        let parsed_source = option
            .desktop_entry_path
            .as_deref()
            .and_then(Self::parse_desktop_entry);
        let name = parsed_source
            .as_ref()
            .and_then(|entry| entry.name.clone())
            .unwrap_or_else(|| option.name.clone());
        let exec = option
            .exec
            .clone()
            .or_else(|| parsed_source.as_ref().and_then(|entry| entry.exec.clone()))
            .or_else(|| {
                option.desktop_entry_id.as_deref().map(|id| {
                    let desktop_id = id.strip_suffix(".desktop").unwrap_or(id);
                    format!("gtk-launch {desktop_id}")
                })
            })
            .unwrap_or_default();
        if exec.trim().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "desktop entry has no executable command",
            ));
        }

        let mut desktop_file = String::new();
        desktop_file.push_str("[Desktop Entry]\n");
        desktop_file.push_str("Type=Application\n");
        desktop_file.push_str("Version=1.0\n");
        desktop_file.push_str(&format!("Name={}\n", Self::sanitize_desktop_value(&name)));
        desktop_file.push_str(&format!("Exec={}\n", Self::sanitize_desktop_value(&exec)));
        if let Some(icon_name) = parsed_source.and_then(|entry| entry.icon) {
            if !icon_name.trim().is_empty() {
                desktop_file.push_str(&format!(
                    "Icon={}\n",
                    Self::sanitize_desktop_value(icon_name.as_str())
                ));
            }
        }
        desktop_file.push_str("X-GNOME-Autostart-enabled=true\n");
        desktop_file.push_str("NoDisplay=false\n");
        desktop_file.push_str("Hidden=false\n");

        fs::write(target, desktop_file)
    }

    fn write_custom_autostart_entry_template() -> std::io::Result<PathBuf> {
        let autostart_dir = Self::user_autostart_dir();
        fs::create_dir_all(&autostart_dir)?;

        let target = Self::next_available_autostart_target_path(
            &autostart_dir,
            &Self::normalize_desktop_file_name("custom-autostart"),
        );

        let mut desktop_file = String::new();
        desktop_file.push_str("[Desktop Entry]\n");
        desktop_file.push_str("Type=Application\n");
        desktop_file.push_str("Version=1.0\n");
        desktop_file.push_str("Name=Custom Autostart\n");
        desktop_file
            .push_str("Comment=Edit Name and Exec to configure this custom autostart entry\n");
        desktop_file.push_str("Icon=application-x-executable\n");
        desktop_file.push_str("Exec=/usr/bin/true\n");
        desktop_file.push_str("X-GNOME-Autostart-enabled=true\n");
        desktop_file.push_str("NoDisplay=false\n");
        desktop_file.push_str("Hidden=false\n");

        fs::write(&target, desktop_file)?;
        Ok(target)
    }

    fn pick_desktop_file_path() -> std::io::Result<Option<PathBuf>> {
        let zenity_result = Self::pick_desktop_file_with_command(
            "zenity",
            &[
                "--file-selection",
                "--title=Desktop-Datei auswählen",
                "--file-filter=Desktop files | *.desktop",
            ],
        );
        match zenity_result {
            Ok(path) => return Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }

        let kdialog_result = Self::pick_desktop_file_with_command(
            "kdialog",
            &[
                "--title",
                "Desktop-Datei auswählen",
                "--getopenfilename",
                ".",
                "*.desktop|Desktop files (*.desktop)",
            ],
        );
        match kdialog_result {
            Ok(path) => Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "kein Dateiauswahldialog gefunden (zenity/kdialog)",
            )),
            Err(err) => Err(err),
        }
    }

    fn pick_desktop_file_with_command(
        program: &str,
        args: &[&str],
    ) -> std::io::Result<Option<PathBuf>> {
        let output = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()?;

        if output.status.success() {
            let picked = String::from_utf8_lossy(&output.stdout)
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| PathBuf::from(line.trim()));
            return Ok(picked);
        }

        // User cancelled dialogs usually exit with code 1.
        if output.status.code() == Some(1) {
            return Ok(None);
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("{program} dateiauswahl fehlgeschlagen"),
        ))
    }

    fn import_desktop_file_to_autostart(source: &Path) -> std::io::Result<PathBuf> {
        if !source
            .extension()
            .is_some_and(|ext| ext == OsStr::new("desktop"))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ausgewählte Datei ist keine .desktop-Datei",
            ));
        }

        let autostart_dir = Self::user_autostart_dir();
        fs::create_dir_all(&autostart_dir)?;

        let file_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .map(Self::normalize_desktop_file_name)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Dateiname der .desktop-Datei ist ungültig",
                )
            })?;
        let target = Self::next_available_autostart_target_path(&autostart_dir, &file_name);
        fs::copy(source, &target)?;
        Ok(target)
    }

    fn normalize_desktop_file_name(value: &str) -> String {
        let trimmed = value.trim();
        if trimmed.to_ascii_lowercase().ends_with(".desktop") {
            trimmed.to_string()
        } else {
            format!("{trimmed}.desktop")
        }
    }

    fn next_available_autostart_target_path(dir: &Path, file_name: &str) -> PathBuf {
        let normalized = Self::normalize_desktop_file_name(file_name);
        let mut candidate = dir.join(&normalized);
        if !candidate.exists() {
            return candidate;
        }

        let stem = Path::new(&normalized)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("custom-autostart")
            .to_string();
        let ext = Path::new(&normalized)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("desktop");

        let mut counter = 1_u32;
        loop {
            candidate = dir.join(format!("{stem}-{counter}.{ext}"));
            if !candidate.exists() {
                return candidate;
            }
            counter += 1;
        }
    }

    fn remove_autostart_entry(path: &str, force_pkexec: bool) -> std::io::Result<()> {
        let target = PathBuf::from(path);
        if force_pkexec {
            return Self::remove_autostart_entry_with_pkexec(&target);
        }

        match fs::remove_file(&target) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                Self::remove_autostart_entry_with_pkexec(&target)
            }
            Err(err) => Err(err),
        }
    }

    fn remove_autostart_entry_with_pkexec(path: &Path) -> std::io::Result<()> {
        let status = Command::new("pkexec")
            .args(["rm", "-f"])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("pkexec konnte nicht gestartet werden: {err}"),
                )
            })?;

        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Entfernen mit pkexec ist fehlgeschlagen",
            ))
        }
    }

    fn user_autostart_dir() -> PathBuf {
        if let Ok(xdg_home) = env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg_home).join("autostart")
        } else if let Ok(home) = env::var("HOME") {
            PathBuf::from(home).join(".config").join("autostart")
        } else {
            PathBuf::from(".config").join("autostart")
        }
    }

    fn autostart_dirs() -> Vec<PathBuf> {
        let mut dirs = vec![Self::user_autostart_dir()];

        let xdg_config_dirs =
            env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_string());
        for dir in xdg_config_dirs
            .split(':')
            .filter(|entry| !entry.trim().is_empty())
        {
            dirs.push(PathBuf::from(dir).join("autostart"));
        }

        dirs
    }

    fn parse_desktop_entry(path: &Path) -> Option<ParsedDesktopEntry> {
        let content = fs::read_to_string(path).ok()?;
        let mut parsed = ParsedDesktopEntry {
            autostart_enabled: true,
            ..ParsedDesktopEntry::default()
        };
        let mut in_desktop_entry = false;

        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
                continue;
            }
            if !in_desktop_entry {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();

            match key {
                "Name" => {
                    if !value.is_empty() {
                        parsed.name = Some(value.to_string());
                    }
                }
                "Exec" => {
                    if !value.is_empty() {
                        parsed.exec = Some(value.to_string());
                    }
                }
                "Icon" => {
                    if !value.is_empty() {
                        parsed.icon = Some(value.to_string());
                    }
                }
                "Hidden" => parsed.hidden = Self::parse_desktop_bool(value),
                "NoDisplay" => parsed.no_display = Self::parse_desktop_bool(value),
                "X-GNOME-Autostart-enabled" => {
                    parsed.autostart_enabled = Self::parse_desktop_bool(value)
                }
                "OnlyShowIn" => parsed.only_show_in = Self::parse_desktop_list(value),
                "NotShowIn" => parsed.not_show_in = Self::parse_desktop_list(value),
                _ => {}
            }
        }

        Some(parsed)
    }

    fn desktop_entry_matches_current(
        entry: &ParsedDesktopEntry,
        current_desktops: &[String],
    ) -> bool {
        if !entry.only_show_in.is_empty()
            && !entry.only_show_in.iter().any(|needle| {
                current_desktops
                    .iter()
                    .any(|current| current.eq_ignore_ascii_case(needle))
            })
        {
            return false;
        }

        !entry.not_show_in.iter().any(|needle| {
            current_desktops
                .iter()
                .any(|current| current.eq_ignore_ascii_case(needle))
        })
    }

    fn current_desktops() -> Vec<String> {
        env::var("XDG_CURRENT_DESKTOP")
            .map(|value| {
                value
                    .split(':')
                    .filter(|segment| !segment.trim().is_empty())
                    .map(|segment| segment.trim().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    fn parse_desktop_bool(raw: &str) -> bool {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes"
        )
    }

    fn parse_desktop_list(raw: &str) -> Vec<String> {
        raw.split(';')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect()
    }

    fn sanitize_desktop_value(value: &str) -> String {
        value
            .replace('\n', " ")
            .replace('\r', " ")
            .trim()
            .to_string()
    }
}
