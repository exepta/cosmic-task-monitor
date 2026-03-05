// SPDX-License-Identifier: MPL-2.0

use super::*;

impl AppModel {
    pub(super) fn performance_view(&self, space_s: u16) -> Element<'_, Message> {
        let cpu_usage = self.system.global_cpu_usage().clamp(0.0, 100.0);
        let avg_freq_mhz = if self.system.cpus().is_empty() {
            0_u64
        } else {
            self.system
                .cpus()
                .iter()
                .map(|cpu| cpu.frequency())
                .sum::<u64>()
                / self.system.cpus().len() as u64
        };
        let current_speed_mhz = Self::read_current_cpu_speed_mhz().unwrap_or(avg_freq_mhz);
        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory().min(total_memory);
        let ram_usage = if total_memory > 0 {
            (used_memory as f32 / total_memory as f32 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let gpu_usage = self
            .gpu_runtime_info
            .utilization_percent
            .or_else(|| self.gpu_usage_history.last().copied());
        let mut active_networks = self.network_interfaces.clone();
        active_networks.sort_by(|a, b| a.name.cmp(&b.name));

        let cpu_card = self.performance_selector_card(
            fl!("table-cpu"),
            format!("{cpu_usage:.1}%"),
            Some(format!("{} GHz", Self::format_ghz(current_speed_mhz))),
            CPU_ACCENT,
            self.performance_view_mode == PerformanceViewMode::Cpu,
            Some(Message::SetPerformanceViewMode(PerformanceViewMode::Cpu)),
        );
        let ram_card = self.performance_selector_card(
            fl!("table-ram"),
            format!(
                "{} / {} ({ram_usage:.0}%)",
                Self::format_rss(used_memory),
                Self::format_rss(total_memory)
            ),
            None,
            RAM_ACCENT,
            self.performance_view_mode == PerformanceViewMode::Ram,
            Some(Message::SetPerformanceViewMode(PerformanceViewMode::Ram)),
        );
        let gpu_card = self.performance_selector_card(
            fl!("table-gpu"),
            gpu_usage
                .map(|value| format!("{value:.1}%"))
                .unwrap_or_else(|| fl!("gpu-not-available")),
            None,
            GPU_ACCENT,
            self.performance_view_mode == PerformanceViewMode::Gpu,
            Some(Message::SetPerformanceViewMode(PerformanceViewMode::Gpu)),
        );

        let mut grouped_disks = self.collect_disk_groups();
        grouped_disks.sort_by(|a, b| a.name.cmp(&b.name));

        let mut sidebar =
            widget::column::with_capacity(4 + active_networks.len() + grouped_disks.len())
                .push(widget::text::title2(fl!("nav-performance")))
                .push(cpu_card)
                .push(ram_card)
                .push(gpu_card)
                .spacing(space_s);

        for network in &active_networks {
            let rx_now = self
                .network_rx_history
                .get(&network.name)
                .and_then(|history| history.last().copied())
                .unwrap_or(0.0);
            let tx_now = self
                .network_tx_history
                .get(&network.name)
                .and_then(|history| history.last().copied())
                .unwrap_or(0.0);
            let mode = PerformanceViewMode::Network(network.name.clone());
            let is_selected = self.performance_view_mode == mode;

            sidebar = sidebar.push(self.network_selector_card(
                network.name.clone(),
                if network.is_wireless {
                    fl!("network-wireless")
                } else {
                    fl!("network-wired")
                },
                format!(
                    "↓ {} • ↑ {}",
                    Self::format_rate_mib(rx_now),
                    Self::format_rate_mib(tx_now)
                ),
                network.is_wireless,
                is_selected,
                Some(Message::SetPerformanceViewMode(mode)),
            ));
        }

        for disk in &grouped_disks {
            let usage = if disk.total_bytes > 0 {
                (disk.used_bytes as f32 / disk.total_bytes as f32 * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            let mode = PerformanceViewMode::Disk(disk.name.clone());
            let is_selected = self.performance_view_mode == mode;
            let is_usb = disk.kind_label.to_ascii_lowercase().contains("usb");

            sidebar = sidebar.push(self.disk_selector_card(
                format!("Disk {}", disk.name),
                disk.kind_label.clone(),
                format!(
                    "{} / {} ({usage:.0}%)",
                    Self::format_rss(disk.used_bytes),
                    Self::format_rss(disk.total_bytes)
                ),
                disk.is_mounted,
                is_usb,
                is_selected,
                Some(Message::SetPerformanceViewMode(mode)),
            ));
        }

        let sidebar = sidebar.width(Length::Fill);

        let detail: Element<'_, Message> = match &self.performance_view_mode {
            PerformanceViewMode::Cpu => self.cpu_detail_panel(cpu_usage, space_s),
            PerformanceViewMode::Ram => {
                self.ram_detail_panel(used_memory, total_memory, ram_usage, space_s)
            }
            PerformanceViewMode::Gpu => self.gpu_detail_panel(gpu_usage, space_s),
            PerformanceViewMode::Network(selected_iface) => {
                if let Some(interface) = active_networks
                    .iter()
                    .find(|interface| &interface.name == selected_iface)
                {
                    self.network_detail_panel(interface, space_s)
                } else if let Some(interface) = active_networks.first() {
                    self.network_detail_panel(interface, space_s)
                } else {
                    widget::container(widget::text(fl!("network-no-active")))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
            }
            PerformanceViewMode::Disk(selected_disk) => {
                if let Some(disk) = grouped_disks
                    .iter()
                    .find(|disk| &disk.name == selected_disk)
                {
                    self.disk_detail_panel(
                        disk.name.as_str(),
                        disk.total_bytes,
                        disk.used_bytes,
                        disk.kind_label.clone(),
                        disk.is_mounted,
                        disk.is_system_disk,
                        &disk.partitions,
                        space_s,
                    )
                } else if let Some(disk) = grouped_disks.first() {
                    self.disk_detail_panel(
                        disk.name.as_str(),
                        disk.total_bytes,
                        disk.used_bytes,
                        disk.kind_label.clone(),
                        disk.is_mounted,
                        disk.is_system_disk,
                        &disk.partitions,
                        space_s,
                    )
                } else {
                    widget::container(widget::text("No disks found"))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
            }
        };

        widget::row::with_capacity(2)
            .push(
                widget::container(widget::scrollable(sidebar).height(Length::Fill))
                    .width(Length::FillPortion(2))
                    .height(Length::Fill),
            )
            .push(widget::container(detail).width(Length::FillPortion(5)))
            .spacing(space_s)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn performance_selector_card(
        &self,
        title: String,
        value: String,
        value_suffix: Option<String>,
        accent: Color,
        is_selected: bool,
        on_press: Option<Message>,
    ) -> widget::Button<'_, Message> {
        let value_row: Element<'_, Message> = if let Some(suffix) = value_suffix {
            widget::row::with_capacity(2)
                .push(widget::text(value).size(14))
                .push(widget::text(suffix).size(14))
                .spacing(15)
                .into()
        } else {
            widget::text(value).size(14).into()
        };

        let mut button = widget::button::custom(
            widget::row::with_capacity(2)
                .push(
                    widget::container(widget::text(""))
                        .class(theme::Container::custom(move |_theme| {
                            widget::container::Style {
                                background: Some(Background::Color(accent)),
                                border: Border {
                                    color: Color::TRANSPARENT,
                                    width: 0.0,
                                    radius: 0.0.into(),
                                },
                                ..Default::default()
                            }
                        }))
                        .width(Length::Fixed(4.0))
                        .height(Length::Fill),
                )
                .push(
                    widget::column::with_capacity(2)
                        .push(widget::text(title).size(20))
                        .push(value_row)
                        .spacing(4)
                        .width(Length::Fill),
                )
                .spacing(12)
                .width(Length::Fill)
                .align_y(Alignment::Center),
        )
        .class(theme::Button::Custom {
            active: Box::new(move |_focused, theme| {
                let mut style = widget::button::Style::new();
                if is_selected {
                    style.background = Some(Background::Color(
                        theme.current_container().component.hover.into(),
                    ));
                    style.border_color = accent;
                } else {
                    style.border_color = theme.cosmic().bg_divider().into();
                }
                style.border_width = 1.0;
                style.border_radius = 10.0.into();
                style
            }),
            hovered: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color { a: 0.08, ..accent }));
                style.border_width = 1.0;
                style.border_color = accent;
                style.border_radius = 10.0.into();
                style
            }),
            pressed: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color { a: 0.16, ..accent }));
                style.border_width = 1.0;
                style.border_color = accent;
                style.border_radius = 10.0.into();
                style
            }),
            disabled: Box::new(move |_theme| {
                let mut style = widget::button::Style::new();
                style.border_width = 1.0;
                style.border_color = accent;
                style.border_radius = 10.0.into();
                style
            }),
        })
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fixed(110.0));

        if let Some(message) = on_press {
            button = button.on_press(message);
        }

        button
    }

    fn network_selector_card(
        &self,
        title: String,
        network_kind: String,
        throughput_text: String,
        is_wireless: bool,
        is_selected: bool,
        on_press: Option<Message>,
    ) -> widget::Button<'_, Message> {
        let icon_name = if is_wireless {
            "network-wireless-symbolic"
        } else {
            "network-wired-symbolic"
        };
        let title_row = widget::row::with_capacity(3)
            .push(widget::text(title).size(18))
            .push(widget::horizontal_space())
            .push(
                icon::from_name(icon_name)
                    .icon()
                    .size(14)
                    .class(theme::Svg::custom(|_| cosmic::iced_widget::svg::Style {
                        color: Some(NETWORK_ACCENT),
                    })),
            )
            .width(Length::Fill)
            .align_y(Alignment::Center);

        let mut button = widget::button::custom(
            widget::row::with_capacity(2)
                .push(
                    widget::container(widget::text(""))
                        .class(theme::Container::custom(move |_theme| {
                            widget::container::Style {
                                background: Some(Background::Color(NETWORK_ACCENT)),
                                border: Border {
                                    color: Color::TRANSPARENT,
                                    width: 0.0,
                                    radius: 0.0.into(),
                                },
                                ..Default::default()
                            }
                        }))
                        .width(Length::Fixed(4.0))
                        .height(Length::Fill),
                )
                .push(
                    widget::column::with_capacity(3)
                        .push(title_row)
                        .push(widget::text(network_kind).size(13))
                        .push(widget::text(throughput_text).size(13))
                        .spacing(4)
                        .width(Length::Fill),
                )
                .spacing(12)
                .width(Length::Fill)
                .align_y(Alignment::Center),
        )
        .class(theme::Button::Custom {
            active: Box::new(move |_focused, theme| {
                let mut style = widget::button::Style::new();
                if is_selected {
                    style.background = Some(Background::Color(
                        theme.current_container().component.hover.into(),
                    ));
                    style.border_color = NETWORK_ACCENT;
                } else {
                    style.background = Some(Background::Color(
                        theme.current_container().component.base.into(),
                    ));
                    style.border_color = theme.cosmic().bg_divider().into();
                }
                style.border_width = 1.0;
                style.border_radius = 10.0.into();
                style
            }),
            hovered: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color {
                    a: 0.08,
                    ..NETWORK_ACCENT
                }));
                style.border_width = 1.0;
                style.border_color = NETWORK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
            pressed: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color {
                    a: 0.16,
                    ..NETWORK_ACCENT
                }));
                style.border_width = 1.0;
                style.border_color = NETWORK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
            disabled: Box::new(move |_theme| {
                let mut style = widget::button::Style::new();
                style.border_width = 1.0;
                style.border_color = NETWORK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
        })
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fixed(115.0));

        if let Some(message) = on_press {
            button = button.on_press(message);
        }

        button
    }

    fn network_detail_panel(
        &self,
        interface: &NetworkInterfaceInfo,
        space_s: u16,
    ) -> Element<'_, Message> {
        let rx_history = self
            .network_rx_history
            .get(&interface.name)
            .cloned()
            .unwrap_or_default();
        let tx_history = self
            .network_tx_history
            .get(&interface.name)
            .cloned()
            .unwrap_or_default();
        let rx_now = *rx_history.last().unwrap_or(&0.0);
        let tx_now = *tx_history.last().unwrap_or(&0.0);
        let rx_peak = rx_history.iter().copied().fold(0.0, f32::max);
        let tx_peak = tx_history.iter().copied().fold(0.0, f32::max);
        let icon_name = if interface.is_wireless {
            "network-wireless-symbolic"
        } else {
            "network-wired-symbolic"
        };
        let type_text = if interface.is_wireless {
            fl!("network-wireless")
        } else {
            fl!("network-wired")
        };
        let speed_text = interface
            .speed_mbps
            .map(|value| format!("{value} Mbps"))
            .unwrap_or_else(|| fl!("network-not-available"));

        let stat_block = |label: String, value: String, accent: bool| {
            let mut value_text = widget::text(value).size(26);
            if accent {
                value_text = value_text.class(theme::Text::Color(NETWORK_ACCENT));
            }

            widget::column::with_capacity(2)
                .push(widget::text(label).size(14))
                .push(value_text)
                .spacing(2)
                .width(Length::Fill)
        };

        let stats_row_1 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    fl!("network-download"),
                    Self::format_rate_mib(rx_now),
                    true,
                ))
                .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    fl!("network-upload"),
                    Self::format_rate_mib(tx_now),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_row_2 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    fl!("network-download-peak"),
                    Self::format_rate_mib(rx_peak),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    fl!("network-upload-peak"),
                    Self::format_rate_mib(tx_peak),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_row_3 = widget::row::with_capacity(1)
            .push(
                widget::container(stat_block(
                    fl!("network-link-speed"),
                    speed_text.clone(),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .width(Length::Fill);

        let stats_col_1 = widget::column::with_capacity(3)
            .push(stats_row_1)
            .push(stats_row_2)
            .push(stats_row_3)
            .spacing(8)
            .width(Length::FillPortion(1));

        let right_line = |label: String, value: String| {
            widget::row::with_capacity(2)
                .push(
                    widget::text(format!("{label}:"))
                        .size(16)
                        .width(Length::Fixed(120.0)),
                )
                .push(widget::text(value).size(16))
                .spacing(10)
                .width(Length::Shrink)
        };

        let stats_col_2 = widget::column::with_capacity(6)
            .push(right_line(fl!("network-name"), interface.name.clone()))
            .push(right_line(fl!("network-type"), type_text))
            .push(right_line(fl!("network-link-speed"), speed_text))
            .push(right_line(
                fl!("network-rx-total"),
                Self::format_rss(interface.rx_bytes),
            ))
            .push(right_line(
                fl!("network-tx-total"),
                Self::format_rss(interface.tx_bytes),
            ))
            .push(right_line(fl!("network-state"), fl!("network-active")))
            .spacing(6)
            .width(Length::FillPortion(1));

        let stats = widget::row::with_capacity(2)
            .push(stats_col_1)
            .push(stats_col_2)
            .spacing(35)
            .width(Length::Fill);

        let panel = widget::column::with_capacity(9)
            .push(
                widget::row::with_capacity(4)
                    .push(widget::text::title1(format!(
                        "{} {}",
                        fl!("table-network"),
                        interface.name
                    )))
                    .push(widget::horizontal_space())
                    .push(
                        icon::from_name(icon_name)
                            .icon()
                            .size(16)
                            .class(theme::Svg::custom(|_| cosmic::iced_widget::svg::Style {
                                color: Some(NETWORK_ACCENT),
                            })),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(widget::text(fl!("network-download-history")).size(14))
            .push(self.sparkline_solid(&rx_history, NETWORK_ACCENT, 130.0))
            .push(widget::text(fl!("network-upload-history")).size(14))
            .push(self.sparkline_solid(
                &tx_history,
                Color::from_rgb(57.0 / 255.0, 103.0 / 255.0, 150.0 / 255.0),
                130.0,
            ))
            .push(widget::Space::with_height(Length::Fixed(24.0)))
            .push(widget::container(stats).width(Length::Fill))
            .spacing(space_s)
            .width(Length::Fill);

        widget::container(
            widget::scrollable(panel)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .padding(18)
        .class(theme::Container::custom(|theme| widget::container::Style {
            background: Some(Background::Color(
                theme.current_container().component.base.into(),
            )),
            border: Border {
                color: NETWORK_ACCENT,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn disk_selector_card(
        &self,
        title: String,
        disk_kind: String,
        usage_text: String,
        is_mounted: bool,
        is_usb: bool,
        is_selected: bool,
        on_press: Option<Message>,
    ) -> widget::Button<'_, Message> {
        let mut title_row = widget::row::with_capacity(5)
            .push(widget::text(title).size(18))
            .push(widget::horizontal_space())
            .width(Length::Fill)
            .align_y(Alignment::Center);

        if is_usb {
            title_row = title_row.push(
                icon::from_name("drive-harddisk-usb-symbolic")
                    .icon()
                    .size(14)
                    .class(theme::Svg::custom(|_| cosmic::iced_widget::svg::Style {
                        color: Some(DISK_ACCENT),
                    })),
            );
        }

        if is_mounted {
            title_row = title_row.push(
                icon::from_name("folder-open-symbolic")
                    .icon()
                    .size(14)
                    .class(theme::Svg::custom(|_| cosmic::iced_widget::svg::Style {
                        color: Some(DISK_ACCENT),
                    })),
            );
        }

        let mut button = widget::button::custom(
            widget::row::with_capacity(2)
                .push(
                    widget::container(widget::text(""))
                        .class(theme::Container::custom(move |_theme| {
                            widget::container::Style {
                                background: Some(Background::Color(DISK_ACCENT)),
                                border: Border {
                                    color: Color::TRANSPARENT,
                                    width: 0.0,
                                    radius: 0.0.into(),
                                },
                                ..Default::default()
                            }
                        }))
                        .width(Length::Fixed(4.0))
                        .height(Length::Fill),
                )
                .push(
                    widget::column::with_capacity(3)
                        .push(title_row)
                        .push(widget::text(disk_kind).size(13))
                        .push(widget::text(usage_text).size(13))
                        .spacing(4)
                        .width(Length::Fill),
                )
                .spacing(12)
                .width(Length::Fill)
                .align_y(Alignment::Center),
        )
        .class(theme::Button::Custom {
            active: Box::new(move |_focused, theme| {
                let mut style = widget::button::Style::new();
                if is_selected {
                    style.background = Some(Background::Color(
                        theme.current_container().component.hover.into(),
                    ));
                    style.border_color = DISK_ACCENT;
                } else {
                    style.background = Some(Background::Color(
                        theme.current_container().component.base.into(),
                    ));
                    style.border_color = theme.cosmic().bg_divider().into();
                }
                style.border_width = 1.0;
                style.border_radius = 10.0.into();
                style
            }),
            hovered: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color {
                    a: 0.08,
                    ..DISK_ACCENT
                }));
                style.border_width = 1.0;
                style.border_color = DISK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
            pressed: Box::new(move |_focused, _theme| {
                let mut style = widget::button::Style::new();
                style.background = Some(Background::Color(Color {
                    a: 0.16,
                    ..DISK_ACCENT
                }));
                style.border_width = 1.0;
                style.border_color = DISK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
            disabled: Box::new(move |_theme| {
                let mut style = widget::button::Style::new();
                style.border_width = 1.0;
                style.border_color = DISK_ACCENT;
                style.border_radius = 10.0.into();
                style
            }),
        })
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fixed(115.0));

        if let Some(message) = on_press {
            button = button.on_press(message);
        }

        button
    }

    fn disk_detail_panel(
        &self,
        disk_name: &str,
        total: u64,
        used: u64,
        kind_label: String,
        is_mounted: bool,
        is_system_disk: bool,
        partitions: &[String],
        space_s: u16,
    ) -> Element<'_, Message> {
        let read_history = self
            .disk_read_history
            .get(disk_name)
            .cloned()
            .unwrap_or_default();
        let write_history = self
            .disk_write_history
            .get(disk_name)
            .cloned()
            .unwrap_or_default();
        let read_now = *read_history.last().unwrap_or(&0.0);
        let write_now = *write_history.last().unwrap_or(&0.0);
        let runtime_info = self
            .disk_runtime_info
            .get(disk_name)
            .copied()
            .unwrap_or_default();
        let usage = if total > 0 {
            (used as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let used_portion = usage.round().clamp(0.0, 100.0) as u16;
        let free_portion = 100_u16.saturating_sub(used_portion);

        let mut partition_tiles = widget::column::with_capacity(partitions.len().max(1))
            .spacing(8)
            .width(Length::Fill);
        for partition in partitions.iter().cloned() {
            partition_tiles = partition_tiles.push(
                widget::container(widget::text(partition).size(13))
                    .padding([4, 10])
                    .class(theme::Container::custom(move |_theme| {
                        widget::container::Style {
                            background: Some(Background::Color(Color {
                                a: 0.18,
                                ..DISK_ACCENT
                            })),
                            border: Border {
                                color: DISK_ACCENT,
                                width: 1.0,
                                radius: 6.0.into(),
                            },
                            ..Default::default()
                        }
                    })),
            );
        }

        let io_stats = widget::row::with_capacity(2)
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text("Lesen").size(14))
                    .push(
                        widget::text(Self::format_rate_mib(read_now))
                            .size(24)
                            .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .spacing(2)
                    .width(Length::FillPortion(1)),
            )
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text("Schreiben").size(14))
                    .push(
                        widget::text(Self::format_rate_mib(write_now))
                            .size(24)
                            .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .spacing(2)
                    .width(Length::FillPortion(1)),
            )
            .spacing(24)
            .width(Length::Fill);

        let extra_stats = widget::column::with_capacity(4)
            .push(widget::text(format!(
                "Systemdatenträger: {}",
                if is_system_disk { "Ja" } else { "Nein" }
            )))
            .push(widget::text(format!("Type: {kind_label}")))
            .push(widget::text(format!(
                "Aktive Zeit: {:.1}%",
                runtime_info.active_time_percent
            )))
            .push(widget::text(format!(
                "Antwortzeit (Durchschnitt): {:.1} ms",
                runtime_info.avg_response_ms
            )))
            .spacing(6)
            .width(Length::Fill);

        let usage_bar = widget::container(
            widget::row::with_capacity(2)
                .push(
                    widget::container(widget::text(""))
                        .class(theme::Container::custom(move |_theme| {
                            widget::container::Style {
                                background: Some(Background::Color(Color {
                                    a: 0.55,
                                    ..DISK_ACCENT
                                })),
                                ..Default::default()
                            }
                        }))
                        .height(Length::Fill)
                        .width(Length::FillPortion(used_portion.max(1))),
                )
                .push(widget::horizontal_space().width(Length::FillPortion(free_portion.max(1))))
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .padding(2)
        .class(theme::Container::custom(move |_theme| {
            widget::container::Style {
                border: Border {
                    color: DISK_ACCENT,
                    width: 2.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        }))
        .height(Length::Fixed(150.0))
        .width(Length::Fill);

        let usage_labels = widget::row::with_capacity(2)
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text("Momentan belegt").size(13))
                    .push(
                        widget::text(Self::format_rss(used))
                            .size(20)
                            .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .spacing(4)
                    .width(Length::FillPortion(1)),
            )
            .push(widget::horizontal_space())
            .push(
                widget::column::with_capacity(2)
                    .push(widget::text("Maximal").size(13))
                    .push(
                        widget::text(Self::format_rss(total))
                            .size(20)
                            .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .align_x(Horizontal::Right)
                    .spacing(4)
                    .width(Length::FillPortion(1)),
            )
            .align_y(Alignment::End)
            .width(Length::Fill);

        let disk_actions: Element<'_, Message> = if is_mounted {
            if is_system_disk {
                widget::container(widget::text(
                    "Systemdatenträger kann nicht ausgehängt werden",
                ))
                .width(Length::Fill)
                .into()
            } else {
                widget::button::standard("Unmount")
                    .class(theme::Button::Suggested)
                    .on_press(Message::UnmountDisk(disk_name.to_string()))
                    .into()
            }
        } else {
            widget::button::standard("Mounten")
                .class(theme::Button::Suggested)
                .on_press(Message::MountDisk(disk_name.to_string()))
                .into()
        };

        let panel = widget::column::with_capacity(8)
            .push(
                widget::row::with_capacity(3)
                    .push(widget::text::title1(format!("Disk {disk_name}")))
                    .push(widget::horizontal_space())
                    .push(
                        widget::text(if is_mounted {
                            kind_label.clone()
                        } else {
                            format!("{kind_label} • Unmounted")
                        })
                        .size(14)
                        .class(theme::Text::Color(DISK_ACCENT)),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(usage_bar)
            .push(usage_labels)
            .push(self.sparkline_solid(&read_history, DISK_ACCENT, 130.0))
            .push(self.sparkline_solid(
                &write_history,
                Color::from_rgb(158.0 / 255.0, 158.0 / 255.0, 54.0 / 255.0),
                130.0,
            ))
            .push(io_stats)
            .push(extra_stats)
            .push(widget::text("Partitionen").size(14))
            .push(partition_tiles)
            .push(widget::Space::with_height(Length::Fixed(12.0)))
            .push(widget::container(disk_actions).width(Length::Shrink))
            .width(Length::Fill)
            .spacing(space_s);

        widget::container(
            widget::scrollable(panel)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .padding(18)
        .class(theme::Container::custom(|theme| widget::container::Style {
            background: Some(Background::Color(
                theme.current_container().component.base.into(),
            )),
            border: Border {
                color: DISK_ACCENT,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn cpu_detail_panel(&self, cpu_usage: f32, space_s: u16) -> Element<'_, Message> {
        let cores = self.system.cpus();
        let cpu_brand = cores.first().map_or("CPU", |cpu| cpu.brand());
        let avg_freq_mhz = if cores.is_empty() {
            0_u64
        } else {
            cores.iter().map(|cpu| cpu.frequency()).sum::<u64>() / cores.len() as u64
        };
        let current_speed_mhz = Self::read_current_cpu_speed_mhz().unwrap_or(avg_freq_mhz);
        let base_freq_mhz = cores.iter().map(|cpu| cpu.frequency()).max().unwrap_or(0);
        let process_count = self.system.processes().len();
        let thread_count = self
            .system
            .processes()
            .values()
            .map(|process| process.tasks().map_or(1_usize, |tasks| tasks.len().max(1)))
            .sum::<usize>();
        let logical_cores = cores.len();
        let uptime = Self::format_uptime(System::uptime());
        let cpu_temp_text = Self::read_cpu_temperature_celsius()
            .map(Self::format_temp_c)
            .unwrap_or_else(|| "N/A".to_string());

        let core_grid = widget::responsive(move |size| {
            let min_tile_width = 200.0;
            let min_tile_height = 150.0;
            let max_tile_height = 260.0;
            let spacing = space_s as f32;
            let columns = (((size.width + spacing) / (min_tile_width + spacing)).floor() as usize)
                .clamp(1, 6);
            let row_count = (self.cpu_usage_history_per_core.len() + columns - 1) / columns;
            let available_height =
                (size.height - spacing * row_count.saturating_sub(1) as f32).max(min_tile_height);
            let tile_height = if row_count > 0 {
                (available_height / row_count as f32).clamp(min_tile_height, max_tile_height)
            } else {
                min_tile_height
            };
            let graph_height = (tile_height - 72.0).clamp(70.0, 160.0);

            let mut rows = widget::column::with_capacity(
                (self.cpu_usage_history_per_core.len() + columns - 1) / columns,
            )
            .spacing(space_s)
            .width(Length::Fill);

            for (row_index, chunk) in self.cpu_usage_history_per_core.chunks(columns).enumerate() {
                let mut row = widget::row::with_capacity(columns)
                    .spacing(space_s)
                    .width(Length::Fill);
                let base_index = row_index * columns;

                for (offset, history) in chunk.iter().enumerate() {
                    let index = base_index + offset;
                    let current_usage = cores.get(index).map_or(0.0, |core| core.cpu_usage());

                    let card = widget::container(
                        widget::column::with_capacity(3)
                            .push(widget::text(format!("Core {}", index + 1)).size(14))
                            .push(
                                widget::text(format!("{current_usage:.1}%"))
                                    .size(16)
                                    .class(theme::Text::Color(CPU_ACCENT)),
                            )
                            .push(self.sparkline(history, CPU_ACCENT, graph_height))
                            .spacing(6)
                            .width(Length::Fill),
                    )
                    .padding(10)
                    .class(theme::Container::custom(|theme| widget::container::Style {
                        background: Some(Background::Color(
                            theme.current_container().component.base.into(),
                        )),
                        border: Border {
                            color: theme.cosmic().bg_divider().into(),
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }))
                    .width(Length::FillPortion(1))
                    .height(Length::Fixed(tile_height));

                    row = row.push(card);
                }

                rows = rows.push(row);
            }

            widget::scrollable(rows).height(Length::Fill).into()
        });

        let stat_block = |label: String, value: String, accent: bool| {
            let mut value_text = widget::text(value).size(26);
            if accent {
                value_text = value_text.class(theme::Text::Color(CPU_ACCENT));
            }

            widget::column::with_capacity(2)
                .push(widget::text(label).size(14))
                .push(value_text)
                .spacing(2)
                .width(Length::Fill)
        };

        let stats_row_1 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    "Last".to_string(),
                    format!("{cpu_usage:.0}%"),
                    true,
                ))
                .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    "Speed".to_string(),
                    format!("{} GHz", Self::format_ghz(current_speed_mhz)),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_row_2 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    "Processes".to_string(),
                    process_count.to_string(),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    "Threads".to_string(),
                    thread_count.to_string(),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_row_3 = widget::row::with_capacity(1)
            .push(
                widget::container(stat_block("Uptime".to_string(), uptime, false))
                    .width(Length::FillPortion(1)),
            )
            .width(Length::Fill);

        let stats_col_1 = widget::column::with_capacity(3)
            .push(stats_row_1)
            .push(stats_row_2)
            .push(stats_row_3)
            .spacing(8)
            .width(Length::FillPortion(1));

        let right_line = |label: &str, value: String| {
            widget::row::with_capacity(2)
                .push(
                    widget::text(format!("{label}:"))
                        .size(16)
                        .width(Length::Fixed(120.0)),
                )
                .push(widget::text(value).size(16))
                .spacing(10)
                .width(Length::Shrink)
        };

        let stats_col_2 = widget::column::with_capacity(3)
            .push(right_line(
                "Base speed",
                format!("{} GHz", Self::format_ghz(base_freq_mhz)),
            ))
            .push(right_line("Cores", logical_cores.to_string()))
            .push(right_line(fl!("stat-temperature").as_str(), cpu_temp_text))
            .push(right_line(
                "Virtualization",
                self.cpu_static_info.virtualization.clone(),
            ))
            .push(right_line(
                "L1 Cache",
                self.cpu_static_info.l1_cache.clone(),
            ))
            .push(right_line(
                "L2 Cache",
                self.cpu_static_info.l2_cache.clone(),
            ))
            .push(right_line(
                "L3 Cache",
                self.cpu_static_info.l3_cache.clone(),
            ))
            .spacing(6)
            .width(Length::FillPortion(1));

        let stats = widget::row::with_capacity(2)
            .push(stats_col_1)
            .push(stats_col_2)
            .spacing(35)
            .width(Length::Fill);

        let panel = widget::column::with_capacity(6)
            .push(
                widget::row::with_capacity(3)
                    .push(widget::text::title1("CPU"))
                    .push(widget::horizontal_space())
                    .push(
                        widget::text(cpu_brand)
                            .size(14)
                            .class(theme::Text::Color(CPU_ACCENT)),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(widget::text("% Auslastung uber 60 Sekunden").size(14))
            .push(core_grid)
            .push(widget::Space::with_height(Length::Fixed(50.0)))
            .push(widget::container(stats).width(Length::Fill))
            .spacing(space_s);

        widget::container(panel)
            .padding(18)
            .class(theme::Container::custom(|theme| widget::container::Style {
                background: Some(Background::Color(
                    theme.current_container().component.base.into(),
                )),
                border: Border {
                    color: CPU_ACCENT,
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn ram_detail_panel(
        &self,
        used_memory: u64,
        total_memory: u64,
        _ram_usage: f32,
        space_s: u16,
    ) -> Element<'_, Message> {
        let available_memory = self.system.available_memory();
        let cached_memory = self.system.free_memory();
        let used_swap = self.system.used_swap();
        let total_swap = self.system.total_swap();

        let stat_block = |label: String, value: String, accent: bool| {
            let mut value_text = widget::text(value).size(26);
            if accent {
                value_text = value_text.class(theme::Text::Color(RAM_ACCENT));
            }

            widget::column::with_capacity(2)
                .push(widget::text(label).size(14))
                .push(value_text)
                .spacing(2)
                .width(Length::Fill)
        };

        let stats_row_1 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    "In use".to_string(),
                    Self::format_rss(used_memory),
                    true,
                ))
                .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    "Available".to_string(),
                    Self::format_rss(available_memory),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_row_2 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    "Cached".to_string(),
                    Self::format_rss(cached_memory),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    "Swap used".to_string(),
                    if total_swap > 0 {
                        format!(
                            "{} / {}",
                            Self::format_rss(used_swap),
                            Self::format_rss(total_swap)
                        )
                    } else {
                        "N/A".to_string()
                    },
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_col_1 = widget::column::with_capacity(2)
            .push(stats_row_1)
            .push(stats_row_2)
            .spacing(8)
            .width(Length::Fill);

        let stats = widget::row::with_capacity(1)
            .push(stats_col_1)
            .width(Length::Fill);

        let panel = widget::column::with_capacity(7)
            .push(
                widget::row::with_capacity(3)
                    .push(widget::text::title1("Memory"))
                    .push(widget::horizontal_space())
                    .push(
                        widget::text(Self::format_rss(total_memory))
                            .size(16)
                            .class(theme::Text::Color(RAM_ACCENT)),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(widget::text("Speicherauslastung").size(14))
            .push(self.sparkline_solid(&self.ram_usage_history, RAM_ACCENT, 240.0))
            .push(
                widget::row::with_capacity(3)
                    .push(
                        widget::column::with_capacity(2)
                            .push(widget::text("Momentan").size(14))
                            .push(
                                widget::text(Self::format_rss(used_memory))
                                    .size(20)
                                    .class(theme::Text::Color(RAM_ACCENT)),
                            )
                            .spacing(2),
                    )
                    .push(widget::horizontal_space())
                    .push(
                        widget::column::with_capacity(2)
                            .push(widget::text("Maximal").size(14))
                            .push(
                                widget::text(Self::format_rss(total_memory))
                                    .size(20)
                                    .class(theme::Text::Color(RAM_ACCENT)),
                            )
                            .spacing(2)
                            .align_x(Horizontal::Right),
                    )
                    .align_y(Alignment::End)
                    .width(Length::Fill),
            )
            .push(widget::Space::with_height(Length::Fixed(50.0)))
            .push(widget::container(stats).width(Length::Fill))
            .spacing(space_s);

        widget::container(
            widget::scrollable(panel)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .padding(18)
        .class(theme::Container::custom(|theme| widget::container::Style {
            background: Some(Background::Color(
                theme.current_container().component.base.into(),
            )),
            border: Border {
                color: RAM_ACCENT,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn gpu_detail_panel(&self, gpu_usage: Option<f32>, space_s: u16) -> Element<'_, Message> {
        let usage_text =
            gpu_usage.map_or_else(|| fl!("gpu-not-available"), |value| format!("{value:.1}%"));
        let vram_used_text = self
            .gpu_runtime_info
            .vram_used_bytes
            .map_or_else(|| fl!("gpu-not-available"), Self::format_rss);
        let vram_total_text = self
            .gpu_runtime_info
            .vram_total_bytes
            .map_or_else(|| fl!("gpu-not-available"), Self::format_rss);
        let vram_combined = match (
            self.gpu_runtime_info.vram_used_bytes,
            self.gpu_runtime_info.vram_total_bytes,
        ) {
            (Some(used), Some(total)) if total > 0 => {
                format!("{} / {}", Self::format_rss(used), Self::format_rss(total))
            }
            _ => fl!("gpu-not-available"),
        };
        let current_speed_text = self.gpu_runtime_info.current_clock_mhz.map_or_else(
            || fl!("gpu-not-available"),
            |mhz| format!("{} GHz", Self::format_ghz(mhz)),
        );
        let max_speed_text = self.gpu_runtime_info.max_clock_mhz.map_or_else(
            || fl!("gpu-not-available"),
            |mhz| format!("{} GHz", Self::format_ghz(mhz)),
        );
        let gpu_temp_text = self
            .gpu_runtime_info
            .temperature_celsius
            .map(Self::format_temp_c)
            .unwrap_or_else(|| fl!("gpu-not-available"));
        let mesa_version_text = self
            .gpu_runtime_info
            .mesa_version
            .clone()
            .unwrap_or_else(|| fl!("gpu-not-available"));

        let stat_block = |label: String, value: String, accent: bool| {
            let mut value_text = widget::text(value).size(26);
            if accent {
                value_text = value_text.class(theme::Text::Color(GPU_ACCENT));
            }

            widget::column::with_capacity(2)
                .push(widget::text(label).size(14))
                .push(value_text)
                .spacing(2)
                .width(Length::Fill)
        };

        let stats_row_1 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(fl!("gpu-stat-last"), usage_text.clone(), true))
                    .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    fl!("gpu-speed"),
                    current_speed_text.clone(),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_row_2 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(fl!("gpu-stat-vram-used"), vram_used_text, false))
                    .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    fl!("gpu-stat-vram-total"),
                    vram_total_text,
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_row_3 = widget::row::with_capacity(2)
            .push(
                widget::container(stat_block(
                    fl!("gpu-speed-max"),
                    max_speed_text.clone(),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .push(
                widget::container(stat_block(
                    fl!("stat-temperature"),
                    gpu_temp_text.clone(),
                    false,
                ))
                .width(Length::FillPortion(1)),
            )
            .spacing(20)
            .width(Length::Fill);

        let stats_col_1 = widget::column::with_capacity(3)
            .push(stats_row_1)
            .push(stats_row_2)
            .push(stats_row_3)
            .spacing(8)
            .width(Length::FillPortion(1));

        let right_line = |label: String, value: String| {
            widget::row::with_capacity(2)
                .push(
                    widget::text(format!("{label}:"))
                        .size(16)
                        .width(Length::Fixed(120.0)),
                )
                .push(widget::text(value).size(16))
                .spacing(10)
                .width(Length::Shrink)
        };

        let stats_col_2 = widget::column::with_capacity(9)
            .push(right_line(
                fl!("gpu-name"),
                self.gpu_runtime_info.name.clone(),
            ))
            .push(right_line(
                fl!("gpu-provider"),
                self.gpu_runtime_info.provider.clone(),
            ))
            .push(right_line(
                fl!("gpu-driver"),
                self.gpu_runtime_info.driver.clone(),
            ))
            .push(right_line(fl!("gpu-mesa"), mesa_version_text))
            .push(right_line(fl!("gpu-vram"), vram_combined))
            .push(right_line(
                fl!("gpu-current-utilization"),
                usage_text.clone(),
            ))
            .push(right_line(fl!("gpu-speed"), current_speed_text))
            .push(right_line(fl!("gpu-speed-max"), max_speed_text))
            .push(right_line(fl!("stat-temperature"), gpu_temp_text))
            .spacing(6)
            .width(Length::FillPortion(1));

        let stats = widget::row::with_capacity(2)
            .push(stats_col_1)
            .push(stats_col_2)
            .spacing(35)
            .width(Length::Fill);

        let mut panel = widget::column::with_capacity(9)
            .push(
                widget::row::with_capacity(3)
                    .push(widget::text::title1(fl!("table-gpu")))
                    .push(widget::horizontal_space())
                    .push(
                        widget::text(self.gpu_runtime_info.name.clone())
                            .size(14)
                            .class(theme::Text::Color(GPU_ACCENT)),
                    )
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(widget::text(fl!("gpu-current-utilization")).size(14))
            .spacing(space_s);

        if self.gpu_usage_history.is_empty() {
            panel = panel.push(widget::text(fl!("gpu-monitoring-unavailable")).size(14));
        } else {
            panel = panel.push(self.sparkline_solid(&self.gpu_usage_history, GPU_ACCENT, 160.0));
        }

        panel = panel.push(widget::text(fl!("gpu-vram-history")).size(14));

        if self.gpu_vram_usage_history.is_empty() {
            panel = panel.push(widget::text(fl!("gpu-vram-monitoring-unavailable")).size(14));
        } else {
            panel =
                panel.push(self.sparkline_solid(&self.gpu_vram_usage_history, RAM_ACCENT, 140.0));
        }

        panel = panel.push(widget::Space::with_height(Length::Fixed(24.0)));
        panel = panel.push(widget::container(stats).width(Length::Fill));
        panel = panel.width(Length::Fill);

        widget::container(
            widget::scrollable(panel)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .padding(18)
        .class(theme::Container::custom(|theme| widget::container::Style {
            background: Some(Background::Color(
                theme.current_container().component.base.into(),
            )),
            border: Border {
                color: GPU_ACCENT,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn sparkline_solid(&self, samples: &[f32], accent: Color, height: f32) -> Element<'_, Message> {
        let mut bars = widget::row::with_capacity(samples.len().max(1))
            .spacing(0)
            .height(Length::Fixed(height))
            .width(Length::Fill)
            .align_y(Alignment::End);

        if samples.is_empty() {
            bars = bars.push(
                widget::container(widget::text(""))
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        } else {
            for sample in samples {
                let clamped = sample.clamp(0.0, 100.0);
                let bar_height = ((clamped / 100.0) * height).max(1.0);
                let top_space = (height - bar_height).max(0.0);
                bars = bars.push(
                    widget::container(
                        widget::column::with_capacity(2)
                            .push(widget::Space::with_height(Length::Fixed(top_space)))
                            .push(
                                widget::container(widget::text(""))
                                    .class(theme::Container::custom(move |_theme| {
                                        widget::container::Style {
                                            background: Some(Background::Color(Color {
                                                a: 0.78,
                                                ..accent
                                            })),
                                            ..Default::default()
                                        }
                                    }))
                                    .height(Length::Fixed(bar_height))
                                    .width(Length::Fill),
                            )
                            .spacing(0),
                    )
                    .width(Length::FillPortion(1))
                    .height(Length::Fill),
                );
            }
        }

        widget::container(bars)
            .padding(0)
            .class(theme::Container::custom(|theme| widget::container::Style {
                background: Some(Background::Color(
                    theme.current_container().component.base.into(),
                )),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .into()
    }

    fn sparkline(&self, samples: &[f32], accent: Color, height: f32) -> Element<'_, Message> {
        let mut bars = widget::row::with_capacity(samples.len().max(1))
            .spacing(1)
            .height(Length::Fixed(height))
            .width(Length::Fill)
            .align_y(Alignment::End);

        if samples.is_empty() {
            bars = bars.push(
                widget::container(widget::text(""))
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        } else {
            for sample in samples {
                let clamped = sample.clamp(0.0, 100.0);
                let bar_height = ((clamped / 100.0) * height).max(1.0);
                let top_space = (height - bar_height).max(0.0);
                bars = bars.push(
                    widget::container(
                        widget::column::with_capacity(2)
                            .push(widget::Space::with_height(Length::Fixed(top_space)))
                            .push(
                                widget::container(widget::text(""))
                                    .class(theme::Container::custom(move |_theme| {
                                        widget::container::Style {
                                            background: Some(Background::Color(Color {
                                                a: 0.75,
                                                ..accent
                                            })),
                                            ..Default::default()
                                        }
                                    }))
                                    .height(Length::Fixed(bar_height))
                                    .width(Length::Fill),
                            )
                            .spacing(0),
                    )
                    .width(Length::FillPortion(1))
                    .height(Length::Fill),
                );
            }
        }

        widget::container(bars)
            .padding(8)
            .class(theme::Container::custom(|theme| widget::container::Style {
                background: Some(Background::Color(
                    theme.current_container().component.base.into(),
                )),
                border: Border {
                    color: theme.cosmic().bg_divider().into(),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }))
            .width(Length::Fill)
            .height(Length::Fixed(height + 16.0))
            .into()
    }
}
