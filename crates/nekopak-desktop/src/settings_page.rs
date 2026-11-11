//! Settings: appearance, account, transfers, network, quota and about.
use crate::{
    app::{error_text, request, Ctx},
    auth, format,
    generated::palettes,
    i18n::{self, tr, tx},
    nav::Route,
    overlay::{MenuItem, PromptOptions},
    panel::{Event, Row, View},
    settings::{update_account_prefs, update_device_prefs, update_settings},
};
use nekopak_domain::*;
use serde_json::json;

const SECTIONS: [(&str, &str); 6] = [
    ("general", "外观"),
    ("account", "账号"),
    ("transfers", "下载与传输"),
    ("network", "网络"),
    ("quota", "配额"),
    ("about", "关于"),
];

pub fn enter(ctx: &Ctx, route: &Route) {
    let Route::Settings { section } = route else {
        return;
    };
    let section = if SECTIONS.iter().any(|(id, _)| id == section) {
        section.clone()
    } else {
        "general".to_owned()
    };
    let token = ctx.panel_begin({
        let section = section.clone();
        move |ctx, event| {
            let ctx = ctx.clone();
            let section = section.clone();
            ctx.clone()
                .spawn(async move { handle(&ctx, &section, event).await });
        }
    });
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let rows = match section.as_str() {
            "account" => account_rows(&ctx).await,
            "transfers" => transfer_rows(&ctx),
            "network" => network_rows(&ctx),
            "quota" => quota_rows(&ctx),
            "about" => about_rows(&ctx),
            _ => general_rows(&ctx),
        };
        if !ctx.panel_owned(token) {
            return;
        }
        ctx.panel_show(View {
            title: tr("设置"),
            icon: "settings",
            tabs: SECTIONS
                .iter()
                .map(|(id, label)| ((*id).to_owned(), tr(label), String::new(), *id == section))
                .collect(),
            rows,
            ..View::default()
        });
    });
}

fn refresh(ctx: &Ctx) {
    enter(ctx, &ctx.route());
}

fn segment(options: &[(&str, &str)], current: &str) -> Vec<(String, String, bool)> {
    options
        .iter()
        .map(|(id, label)| ((*id).to_owned(), tr(label), *id == current))
        .collect()
}

fn general_rows(ctx: &Ctx) -> Vec<Row> {
    let core = ctx.core.borrow();
    let prefs = ctx.prefs.borrow();
    let theme = core.settings["theme"].as_str().unwrap_or("system");
    let language = core.settings["language"].as_str().unwrap_or("system");
    let language_label = i18n::LANGUAGES
        .iter()
        .find(|(id, _)| *id == language)
        .map(|(_, label)| tr(label))
        .unwrap_or_default();
    let accent = palettes::ACCENTS
        .iter()
        .find(|accent| accent.id == prefs.device.accent)
        .unwrap_or(&palettes::ACCENTS[0]);
    let autostart = crate::autostart::get();
    let mut rows = vec![
        Row::header(tr("外观")),
        Row::segment(
            "theme",
            tr("主题"),
            segment(
                &[("system", "跟随系统"), ("light", "浅色"), ("dark", "深色")],
                theme,
            ),
        )
        .icon("palette"),
        Row::select("accent", tr("强调色"), tr(accent.label)).icon("palette"),
        Row::select("language", tr("界面语言"), language_label).icon("label"),
        Row::segment(
            "density",
            tr("列表行距"),
            segment(
                &[("comfortable", "舒适"), ("compact", "紧凑")],
                &prefs.account.density,
            ),
        )
        .icon("rows"),
        Row::segment(
            "grid-size",
            tr("网格卡片大小"),
            segment(
                &[("s", "小"), ("m", "中"), ("l", "大")],
                &prefs.account.grid_size,
            ),
        )
        .icon("grid")
        .detail(tr("在文件列表里按 Ctrl 加减号也能切换。")),
        Row::toggle(
            "folders-first",
            tr("文件夹排在前面"),
            prefs.account.folders_first,
        )
        .icon("folder"),
        Row::toggle("reduce-motion", tr("减少动画"), prefs.device.reduce_motion).icon("motion"),
        Row::header(tr("行为")),
        Row::toggle(
            "tray",
            tr("关闭窗口时最小化到托盘"),
            core.settings["close_to_tray"] == true,
        )
        .icon("monitor")
        .disabled(!crate::desktop::tray_available()),
    ];
    rows.push(match autostart {
        Ok(Some(enabled)) => Row::toggle("autostart", tr("开机自动启动"), enabled)
            .icon("bolt")
            .detail(tr("仅为当前 Windows 用户设置登录后启动 NekoPak。")),
        Ok(None) => Row::toggle("autostart", tr("开机自动启动"), false)
            .icon("bolt")
            .detail(tr("当前平台尚未支持自动启动"))
            .disabled(true),
        Err(_) => Row::toggle("autostart", tr("开机自动启动"), false)
            .icon("bolt")
            .detail(tr("当前用户自动启动不可用"))
            .disabled(true),
    });
    rows.push(Row::button("shortcuts", tr("快捷键"), tr("查看")).icon("keyboard"));
    rows
}

async fn account_rows(ctx: &Ctx) -> Vec<Row> {
    let mut rows = vec![Row::header(tr("账号"))];
    if ctx.mock {
        rows.push(Row::note(
            tr("离线界面预览：未连接 PikPak，不使用真实账号或密码。"),
            "info",
        ));
        return rows;
    }
    let (auth, auth_error) = {
        let core = ctx.core.borrow();
        (core.auth.clone(), core.auth_error.clone())
    };
    if let Some(error) = auth_error {
        rows.push(Row::note(error_text(&error), "error"));
        rows.push(Row::button("reconnect", tr("重试连接"), tr("重试连接")).icon("refresh"));
    }
    match auth.filter(|auth| auth.authenticated) {
        Some(auth) => {
            let tier = match auth.account_tier {
                AccountTier::Premium => tr("Premium 会员"),
                AccountTier::Free => tr("免费账号"),
                AccountTier::Unknown => String::new(),
            };
            rows.push(
                Row::button(
                    "sign-out",
                    auth.display_name.unwrap_or_else(|| tr("已登录")),
                    tr("退出登录"),
                )
                .icon("user")
                .tone("primary")
                .detail(tier),
            );
        }
        None => {
            let label = if ctx.auth_expired() {
                tr("登录已过期，请重新登录。")
            } else {
                tr("未登录")
            };
            rows.push(Row::button("sign-in", label, tr("登录")).icon("login"));
        }
    }
    if ctx.feature("accounts") {
        let accounts = ctx
            .call(|service| async move { service.accounts_list(&request()).await })
            .await
            .unwrap_or_default();
        if accounts.len() > 1 || accounts.iter().any(|account| !account.active) {
            rows.push(Row::header(tr("已保存的账号")));
            for account in accounts {
                let state = if account.active {
                    tr("当前账号")
                } else if account.state == AuthStatus::Expired {
                    tr("登录已过期")
                } else {
                    String::new()
                };
                let mut row = Row::item(
                    &format!("account:{}", account.id),
                    account.display_name.clone(),
                )
                .icon("user")
                .detail(state)
                .action2(tr("移除账号"));
                if !account.active {
                    row = row.action(tr("切换"));
                }
                rows.push(row.selected(account.active));
            }
        }
        rows.push(Row::button("add-account", tr("添加其他账号"), tr("登录")).icon("plus"));
    }
    rows
}

fn quality_label(quality: &str) -> String {
    if quality == "original" {
        tr("原画（原始文件）")
    } else {
        tx("不高于 {0}", &[quality])
    }
}

fn transfer_rows(ctx: &Ctx) -> Vec<Row> {
    let core = ctx.core.borrow();
    let settings = &core.settings;
    let limit = settings["bandwidth_limit_kib_s"].as_u64().unwrap_or(0);
    let cache = settings["cache"]["max_bytes"]
        .as_str()
        .and_then(|bytes| bytes.parse::<u64>().ok())
        .unwrap_or(0);
    let player = settings["media"]["player_path"]
        .as_str()
        .unwrap_or_default();
    let mut rows = vec![
        Row::header(tr("下载与传输")),
        Row::note(tr("下载原始文件，支持暂停、继续及重启后恢复。"), "info"),
        Row::button("download-dir", tr("下载目录"), tr("选择文件夹"))
            .icon("folder")
            .detail(settings["download_dir"].as_str().unwrap_or_default()),
        Row::toggle(
            "ask-dir",
            tr("下载时询问保存位置"),
            settings["ask_download_directory"] == true,
        )
        .icon("folder-open")
        .detail(tr("每次下载选择文件夹")),
        Row::select(
            "concurrent",
            tr("同时传输数"),
            settings["max_concurrent_transfers"]
                .as_u64()
                .unwrap_or(1)
                .to_string(),
        )
        .icon("transfer")
        .detail(tr("同时进行的任务数，1–8。")),
        Row::select(
            "connections",
            tr("每个任务的连接数"),
            settings["download"]["connections_per_task"]
                .as_u64()
                .unwrap_or(1)
                .to_string(),
        )
        .icon("hub")
        .detail(tr("多个连接分段下载同一个文件，1–16。")),
        Row::select(
            "limit",
            tr("带宽上限（KiB/s）"),
            if limit == 0 {
                tr("不限速")
            } else {
                format::group_digits(limit)
            },
        )
        .icon("speed")
        .detail(tr("0 表示不限速，修改后对后续下载块生效。")),
        Row::select(
            "quality",
            tr("默认下载画质"),
            quality_label(
                settings["download"]["default_quality"]
                    .as_str()
                    .unwrap_or("original"),
            ),
        )
        .icon("quality")
        .detail(tr(
            "下载视频时使用不高于此档的最高画质。右键“下载指定画质…”可临时改变。",
        )),
        Row::header(tr("播放与缓存")),
        Row::button("player", tr("外部播放器"), tr("选择播放器"))
            .icon("play-circle")
            .detail(if player.is_empty() {
                tr("未选择，将使用系统默认播放器")
            } else {
                player.to_owned()
            }),
    ];
    if !player.is_empty() {
        rows.push(Row::button("player-clear", tr("清除播放器配置"), tr("清除")).icon("x"));
    }
    rows.push(
        Row::select(
            "cache",
            tr("缓存上限（GiB）"),
            format!("{:.1}", cache as f64 / 1_073_741_824.0),
        )
        .icon("savings")
        .detail(tr("超出后先清理最久没用的缓存。")),
    );
    rows
}

fn network_rows(ctx: &Ctx) -> Vec<Row> {
    let core = ctx.core.borrow();
    let network = &core.settings["experimental_network"];
    let domain = network["server_domain"].as_str().unwrap_or("auto");
    let mode = network["proxy_mode"].as_str().unwrap_or("system");
    let enabled = network["enabled"] == true;
    let mut rows = vec![
        Row::header(tr("网络")),
        Row::select(
            "domain",
            tr("服务器域名"),
            if domain == "auto" {
                tr("自动选择")
            } else {
                domain.to_owned()
            },
        )
        .icon("network")
        .detail(tr("保存后用于后续请求。测速使用已保存的网络设置。")),
        Row::button("probe", tr("测速"), tr("测速")).icon("speed"),
        Row::button("reconnect", tr("重新连接"), tr("重新连接")).icon("refresh"),
        Row::header(tr("网络增强")),
        Row::toggle("net-enabled", tr("启用网络增强"), enabled)
            .icon("shield")
            .detail(tr("默认关闭。按你的网络环境选择应用内代理或兼容连接。")),
        Row::segment(
            "proxy-mode",
            tr("代理"),
            segment(
                &[
                    ("system", "跟随系统"),
                    ("none", "不使用"),
                    ("user_provided", "自定义"),
                ],
                mode,
            ),
        )
        .icon("hub")
        .disabled(!enabled),
    ];
    if mode == "user_provided" {
        rows.push(
            Row::select(
                "proxy-url",
                tr("代理地址"),
                network["proxy_url"].as_str().unwrap_or_default(),
            )
            .detail(tr("支持 HTTP、HTTPS、SOCKS5；当前不保存代理账号密码。"))
            .disabled(!enabled),
        );
    }
    rows.push(
        Row::toggle("compat", tr("兼容连接"), network["compatibility_mode"] == true)
            .icon("lock")
            .detail(tr("限制认证接口使用 TLS 1.2，用于排查部分网络中的握手重置。保持证书校验；不保证解决所有地区的连接问题。"))
            .disabled(!enabled),
    );
    rows.push(
        Row::toggle(
            "block-access",
            tr("屏蔽 access 域名"),
            network["block_access_domains"] == true,
        )
        .icon("block")
        .detail(tr(
            "默认关闭，独立于网络增强开关。只阻止 NekoPak 访问以下域名，不修改系统 Hosts。",
        )),
    );
    rows.push(Row::note(tr("设置只影响 NekoPak 后续连接。关闭网络增强后恢复系统代理，不修改 Hosts、DNS 或系统代理。"), "info"));
    rows
}

fn quota_rows(ctx: &Ctx) -> Vec<Row> {
    let core = ctx.core.borrow();
    let mut rows = vec![Row::header(tr("配额"))];
    let Some(quota) = &core.quota else {
        rows.push(Row::note(
            if ctx.signed_in() {
                tr("暂时无法获取")
            } else {
                tr("登录后可查询配额。")
            },
            "info",
        ));
        rows.push(Row::button("quota-refresh", tr("刷新"), tr("刷新")).icon("refresh"));
        return rows;
    };
    if ctx.mock {
        rows.push(Row::note(tr("当前为演示数据。"), "info"));
    }
    for dimension in &quota.dimensions {
        let label = tr(match dimension.dimension.as_str() {
            "storage" => "存储容量",
            "download_daily" => "每日下行",
            "download" => "每月下行",
            "upload" => "每月上传",
            "cloud_download" => "每月云下载",
            other => other,
        });
        let limit = dimension.limit_bytes.map(|bytes| bytes.0);
        let remaining = dimension.remaining_bytes.map(|bytes| bytes.0);
        let used = dimension
            .used_bytes
            .map(|bytes| bytes.0)
            .or(match (limit, remaining) {
                (Some(limit), Some(remaining)) => Some(limit.saturating_sub(remaining)),
                _ => None,
            });
        let mut detail = match (used, limit) {
            (Some(used), Some(limit)) => {
                format!("{} / {}", format::bytes(used), format::bytes(limit))
            }
            (Some(used), None) => format!(
                "{}{}",
                tx("{0} 已用", &[&format::bytes(used)]),
                tr("（上限未提供）")
            ),
            _ => tr("不可获取"),
        };
        if let Some(remaining) = remaining {
            detail.push_str(&tx("，剩余 {0}", &[&format::bytes(remaining)]));
        }
        if let Some(resets) = &dimension.resets_at {
            detail.push_str(&format!(
                " · {} {}",
                tr("重置时间"),
                format::date_time(Some(resets))
            ));
        }
        let ratio = match (used, limit) {
            (Some(used), Some(limit)) if limit > 0 => (used as f64 / limit as f64).min(1.0) as f32,
            _ => 0.0,
        };
        let exhausted = remaining == Some(0);
        rows.push(
            Row::progress("", label, ratio)
                .detail(detail)
                .value(if exhausted {
                    tr("已用尽")
                } else if ratio > 0.9 {
                    tr("接近上限")
                } else {
                    String::new()
                })
                .tone(if exhausted { "error" } else { "primary" }),
        );
    }
    for limitation in &quota.limitations {
        rows.push(Row::note(tr(limitation), "info"));
    }
    rows.push(
        Row::button(
            "quota-refresh",
            format!(
                "{} {}",
                tr("更新于"),
                format::relative(Some(&quota.checked_at))
            ),
            tr("刷新"),
        )
        .icon("refresh"),
    );
    rows
}

fn about_rows(ctx: &Ctx) -> Vec<Row> {
    let mut rows = vec![
        Row::header("NekoPak"),
        Row::info(tr("版本"), ctx.version()).icon("info"),
        Row::note(
            tr("NekoPak 是社区开发的 PikPak 客户端，与 PikPak 官方无关。"),
            "info",
        ),
        Row::info(
            tr("界面"),
            tr("原生界面（Slint），不依赖 WebView2 或浏览器内核"),
        )
        .icon("monitor"),
    ];
    rows.extend(crate::updates::rows(ctx));
    rows.push(Row::header(tr("来源与致谢")));
    for (name, relation, license) in [
        (
            "Piko",
            "界面与交互设计参考（圆角岛外框、侧栏、任务页布局），未复制代码或素材",
            "AGPL-3.0",
        ),
        ("pikpaktui", "PikPak 接口适配参考（Rust）", "Apache-2.0"),
        ("pikpakcli", "PikPak 接口行为研究", "MIT"),
        (
            "Rclone",
            "PikPak 接口实现参考（backend/pikpak）；可选外部程序，不捆绑",
            "MIT",
        ),
        (
            "Slint",
            "原生界面工具包（免版税许可）",
            "Slint Royalty-free 2.0",
        ),
        ("Material Symbols", "界面图标", "Apache-2.0"),
    ] {
        rows.push(Row::info(name, license).detail(tr(relation)));
    }
    rows.push(Row::note(
        tr("猫箱标志为 NekoPak 原创；界面使用系统字体，未嵌入字体文件。"),
        "info",
    ));
    rows.push(Row::button("repo", tr("源码仓库与反馈"), tr("打开")).icon("external"));
    rows.push(Row::button("slint", "Made with Slint", tr("打开")).icon("external"));
    rows.push(Row::button("pikpak", tr("PikPak 官网"), tr("打开")).icon("external"));
    rows
}

async fn number(
    ctx: &Ctx,
    title: String,
    hint: String,
    initial: u64,
    min: u64,
    max: u64,
) -> Option<u64> {
    let message = tx(
        "请输入 {0}–{1} 之间的整数",
        &[&min.to_string(), &max.to_string()],
    );
    ctx.prompt(PromptOptions {
        title,
        label: hint,
        initial: initial.to_string(),
        placeholder: String::new(),
        confirm: tr("保存"),
        password: false,
        select_to: 0,
        validate: Box::new(move |text| match text.trim().parse::<u64>() {
            Ok(value) if (min..=max).contains(&value) => (None, String::new()),
            _ => (Some(message.clone()), String::new()),
        }),
    })
    .await
    .and_then(|text| text.trim().parse().ok())
}

async fn pick(
    ctx: &Ctx,
    x: f32,
    y: f32,
    options: Vec<(String, String)>,
    current: &str,
) -> Option<String> {
    let items = options
        .iter()
        .map(|(id, label)| MenuItem::new(id, label.clone()).checked(id == current))
        .collect();
    ctx.menu(x, y, "", items).await
}

async fn handle(ctx: &Ctx, section: &str, event: Event) {
    let (id, what, x, y) = match event {
        Event::Tab(id) => return ctx.replace_route(Route::Settings { section: id }),
        Event::Row(id, what, x, y) => (id, what, x, y),
        _ => return,
    };
    let settings = ctx.core.borrow().settings.clone();
    let saved = match id.as_str() {
        "theme" => update_settings(ctx, json!({ "theme": what })).await,
        "accent" => {
            let options = palettes::ACCENTS
                .iter()
                .map(|accent| (accent.id.to_owned(), tr(accent.label)))
                .collect();
            let current = ctx.prefs.borrow().device.accent.clone();
            match pick(ctx, x, y, options, &current).await {
                Some(choice) => {
                    update_device_prefs(ctx, |prefs| prefs.accent = choice);
                    true
                }
                None => false,
            }
        }
        "language" => {
            let options = i18n::LANGUAGES
                .iter()
                .map(|(id, label)| ((*id).to_owned(), tr(label)))
                .collect();
            match pick(
                ctx,
                x,
                y,
                options,
                settings["language"].as_str().unwrap_or("system"),
            )
            .await
            {
                Some(choice) => {
                    let ok = update_settings(ctx, json!({ "language": choice })).await;
                    ctx.refresh_tabs();
                    ok
                }
                None => false,
            }
        }
        "density" => update_account_prefs(ctx, |prefs| prefs.density = what),
        "grid-size" => update_account_prefs(ctx, |prefs| prefs.grid_size = what),
        "folders-first" => {
            update_account_prefs(ctx, |prefs| prefs.folders_first = !prefs.folders_first)
        }
        "reduce-motion" => {
            update_device_prefs(ctx, |prefs| prefs.reduce_motion = !prefs.reduce_motion);
            true
        }
        "tray" => {
            update_settings(
                ctx,
                json!({ "close_to_tray": settings["close_to_tray"] != true }),
            )
            .await
        }
        "autostart" => {
            let enabled = crate::autostart::get().ok().flatten().unwrap_or(false);
            match crate::autostart::set(!enabled) {
                Ok(_) => true,
                Err(error) => {
                    ctx.toast_error(&error);
                    false
                }
            }
        }
        "shortcuts" => {
            shortcuts(ctx).await;
            false
        }
        "sign-in" | "add-account" => {
            auth::open_sign_in(ctx);
            false
        }
        "sign-out" => {
            auth::sign_out(ctx).await;
            true
        }
        "reconnect" => {
            ctx.reconnect().await;
            ctx.toast(tr("已重新连接"), "success");
            true
        }
        "download-dir" => {
            let current = settings["download_dir"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            match crate::platform::pick_folder(ctx, &tr("选择下载文件夹"), &current).await {
                Some(path) => update_settings(ctx, json!({ "download_dir": path })).await,
                None => false,
            }
        }
        "ask-dir" => {
            update_settings(
                ctx,
                json!({ "ask_download_directory": settings["ask_download_directory"] != true }),
            )
            .await
        }
        "concurrent" => match number(
            ctx,
            tr("同时传输数"),
            tr("同时进行的任务数，1–8。"),
            settings["max_concurrent_transfers"].as_u64().unwrap_or(1),
            1,
            8,
        )
        .await
        {
            Some(value) => update_settings(ctx, json!({ "max_concurrent_transfers": value })).await,
            None => false,
        },
        "connections" => {
            let current = settings["download"]["connections_per_task"]
                .as_u64()
                .unwrap_or(1);
            match number(
                ctx,
                tr("每个任务的连接数"),
                tr("多个连接分段下载同一个文件，1–16。"),
                current,
                1,
                16,
            )
            .await
            {
                Some(value) => {
                    update_settings(
                        ctx,
                        json!({ "download": { "connections_per_task": value } }),
                    )
                    .await
                }
                None => false,
            }
        }
        "limit" => {
            let current = settings["bandwidth_limit_kib_s"].as_u64().unwrap_or(0);
            match number(
                ctx,
                tr("带宽上限（KiB/s）"),
                tr("0 表示不限速，修改后对后续下载块生效。"),
                current,
                0,
                1_048_576,
            )
            .await
            {
                Some(value) => {
                    update_settings(ctx, json!({ "bandwidth_limit_kib_s": value })).await
                }
                None => false,
            }
        }
        "quality" => {
            let options = ["original", "2160p", "1080p", "720p", "480p"]
                .iter()
                .map(|q| ((*q).to_owned(), quality_label(q)))
                .collect();
            match pick(
                ctx,
                x,
                y,
                options,
                settings["download"]["default_quality"]
                    .as_str()
                    .unwrap_or("original"),
            )
            .await
            {
                Some(choice) => {
                    update_settings(ctx, json!({ "download": { "default_quality": choice } })).await
                }
                None => false,
            }
        }
        "player" => {
            match crate::platform::pick_file(ctx, &tr("选择播放器"), ("Application", &["exe"]))
                .await
            {
                Some(path) => {
                    update_settings(
                        ctx,
                        json!({ "media": { "player_path": path.to_string_lossy() } }),
                    )
                    .await
                }
                None => false,
            }
        }
        "player-clear" => update_settings(ctx, json!({ "media": { "player_path": "" } })).await,
        "cache" => {
            let current = settings["cache"]["max_bytes"]
                .as_str()
                .and_then(|bytes| bytes.parse::<u64>().ok())
                .unwrap_or(0)
                / 1_073_741_824;
            match number(
                ctx,
                tr("缓存上限（GiB）"),
                tr("超出后先清理最久没用的缓存。"),
                current.max(1),
                1,
                4096,
            )
            .await
            {
                Some(value) => {
                    update_settings(
                        ctx,
                        json!({ "cache": { "max_bytes": (value * 1_073_741_824).to_string() } }),
                    )
                    .await
                }
                None => false,
            }
        }
        "domain" => {
            let mut options = vec![("auto".to_owned(), tr("自动选择"))];
            options.extend(
                [
                    "mypikpak.com",
                    "mypikpak.net",
                    "pikpak.me",
                    "pikpakdrive.com",
                ]
                .iter()
                .map(|d| ((*d).to_owned(), (*d).to_owned())),
            );
            match pick(
                ctx,
                x,
                y,
                options,
                settings["experimental_network"]["server_domain"]
                    .as_str()
                    .unwrap_or("auto"),
            )
            .await
            {
                Some(choice) => {
                    update_settings(
                        ctx,
                        json!({ "experimental_network": { "server_domain": choice } }),
                    )
                    .await
                }
                None => false,
            }
        }
        "probe" => {
            probe(ctx).await;
            false
        }
        "net-enabled" => {
            let enabled = settings["experimental_network"]["enabled"] != true;
            update_settings(
                ctx,
                json!({ "experimental_network": { "enabled": enabled } }),
            )
            .await
        }
        "proxy-mode" => {
            update_settings(
                ctx,
                json!({ "experimental_network": { "proxy_mode": what } }),
            )
            .await
        }
        "proxy-url" => {
            let current = settings["experimental_network"]["proxy_url"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let value = ctx
                .prompt(PromptOptions {
                    title: tr("代理地址"),
                    label: tr("支持 HTTP、HTTPS、SOCKS5；当前不保存代理账号密码。"),
                    initial: current,
                    placeholder: "socks5h://127.0.0.1:1080".into(),
                    confirm: tr("保存"),
                    password: false,
                    select_to: 0,
                    validate: Box::new(|text| {
                        let lower = text.trim().to_ascii_lowercase();
                        let ok = ["http://", "https://", "socks5://", "socks5h://"]
                            .iter()
                            .any(|scheme| lower.starts_with(scheme))
                            && !lower.contains('@');
                        (
                            if ok {
                                None
                            } else {
                                Some(tr("请输入不含账号密码的 HTTP、HTTPS 或 SOCKS5 地址"))
                            },
                            String::new(),
                        )
                    }),
                })
                .await;
            match value {
                Some(url) => {
                    update_settings(
                        ctx,
                        json!({ "experimental_network": { "proxy_url": url.trim() } }),
                    )
                    .await
                }
                None => false,
            }
        }
        "compat" => {
            let enabled = settings["experimental_network"]["compatibility_mode"] != true;
            update_settings(
                ctx,
                json!({ "experimental_network": { "compatibility_mode": enabled } }),
            )
            .await
        }
        "block-access" => {
            let enabled = settings["experimental_network"]["block_access_domains"] != true;
            update_settings(
                ctx,
                json!({ "experimental_network": { "block_access_domains": enabled } }),
            )
            .await
        }
        "quota-refresh" => {
            crate::settings::refresh_quota(ctx).await;
            true
        }
        "repo" => {
            ctx.open_link("https://github.com/NekoPak/NekoPak");
            false
        }
        "slint" => {
            ctx.open_link("https://slint.dev");
            false
        }
        "pikpak" => {
            ctx.open_link("https://mypikpak.com");
            false
        }
        other => match other.strip_prefix("account:") {
            Some(account) => {
                let accounts = ctx
                    .call(|service| async move { service.accounts_list(&request()).await })
                    .await
                    .unwrap_or_default();
                if let Some(saved) = accounts.into_iter().find(|saved| saved.id == account) {
                    if what == "action" {
                        auth::switch_account(ctx, saved.id).await;
                    } else if what == "action2" {
                        auth::remove_account(ctx, saved).await;
                    }
                }
                true
            }
            None => crate::updates::handle(ctx, other).await,
        },
    };
    if saved
        && matches!(ctx.route(), Route::Settings { section: ref current } if current == section)
    {
        refresh(ctx);
    }
}

async fn probe(ctx: &Ctx) {
    ctx.toast(tr("正在测速…"), "info");
    let result = ctx
        .call(|service| async move {
            service
                .network_domains_probe(&nekopak_provider::RequestContext::new(
                    std::time::Duration::from_secs(60),
                ))
                .await
        })
        .await;
    match result {
        Ok(value) => {
            let rows: Vec<(String, String, bool)> = value["domains"]
                .as_array()
                .or(value.as_array())
                .map(|list| {
                    list.iter()
                        .map(|entry| {
                            let domain = entry["domain"].as_str().unwrap_or_default().to_owned();
                            let result = match entry["latency_ms"].as_u64() {
                                Some(latency) => format!("{latency} ms"),
                                None => tr("不可用"),
                            };
                            (domain, result, false)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if rows.is_empty() {
                ctx.toast(tr("没有可用的测速结果"), "warning");
            } else {
                ctx.details(&tr("测速"), rows, None).await;
            }
        }
        Err(error) => ctx.toast_error(&error),
    }
}

pub async fn shortcuts(ctx: &Ctx) {
    let rows = [
        ("Ctrl+1 … Ctrl+8", "切换页面"),
        ("Ctrl+T / Ctrl+W", "新建 / 关闭标签页"),
        ("Ctrl+Tab", "切换标签页"),
        ("Alt+← / Alt+→", "后退 / 前进"),
        ("Backspace", "上一级"),
        ("Ctrl+F", "搜索"),
        ("F5 / Ctrl+R", "刷新"),
        ("Ctrl+A", "全选"),
        ("Shift + 方向键", "扩展选择"),
        ("Enter / 双击", "打开"),
        ("F2", "重命名"),
        ("Delete", "移至回收站"),
        ("Ctrl+C / Ctrl+X / Ctrl+V", "复制 / 剪切 / 粘贴"),
        ("Ctrl+Shift+C", "复制路径"),
        ("Ctrl+D", "添加或取消星标"),
        ("Ctrl+Shift+N", "新建文件夹"),
        ("Ctrl+Z", "撤销"),
        ("Alt+Enter", "属性"),
        ("Ctrl + / Ctrl -", "切换列表与网格大小"),
        ("Ctrl+B", "展开或收起侧栏"),
        ("Ctrl+,", "设置"),
        ("F1", "快捷键"),
    ];
    ctx.details(
        &tr("快捷键"),
        rows.iter()
            .map(|(keys, label)| ((*keys).to_owned(), tr(label), false))
            .collect(),
        None,
    )
    .await;
}
