//! Media: external playback, quality-capped downloads and time-range clips.
use crate::{
    app::{error_text, new_key, request, Ctx},
    format,
    i18n::{tr, tx},
    nav::Route,
    overlay::{Field, FormEvent},
};
use nekopak_core::TransferOptions;
use nekopak_domain::*;
use serde_json::Value;

/// Opens a local streaming session in the configured player, or the system
/// default application when none is configured. No URL leaves this machine.
pub async fn play(ctx: &Ctx, entry: RemoteEntry) {
    if !entry.id.starts_with("local:") {
        if let Err(reason) = ctx
            .gate(Operation::ExternalPlayer)
            .or_else(|_| ctx.gate(Operation::StreamUrl))
        {
            return ctx.toast(reason, "info");
        }
    }
    let player = ctx.core.borrow().settings["media"]["player_path"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let id = entry.id.clone();
    let session = ctx
        .call(move |service| async move { service.media_session_open(&id, None).await })
        .await;
    let session = match session {
        Ok(session) => session,
        Err(error) => return ctx.toast_error(&error),
    };
    let (Some(url), Some(session_id)) = (session["url"].as_str(), session["session_id"].as_str())
    else {
        return ctx.toast(tr("媒体会话暂不可用。"), "error");
    };
    let launched = if player.is_empty() {
        // Sessions are served from loopback only.
        url.starts_with("http://127.0.0.1:") && open::that_detached(url).is_ok()
    } else {
        let mut command = std::process::Command::new(&player);
        command
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        match command.spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                true
            }
            Err(_) => false,
        }
    };
    if launched {
        ctx.toast(tx("正在用播放器打开“{0}”", &[&entry.name]), "info");
    } else {
        let session_id = session_id.to_owned();
        let _ = ctx
            .call(move |service| async move { service.media_session_close(&session_id) })
            .await;
        ctx.toast_action(
            tr("无法启动所选播放器，请检查应用是否可用。"),
            "error",
            tr("设置"),
            |ctx| {
                ctx.navigate(Route::Settings {
                    section: "transfers".into(),
                })
            },
        );
    }
}

fn quality_choices(probe: &Value) -> Vec<(String, String, String)> {
    probe["qualities"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|quality| {
                    let id = quality["id"].as_str()?.to_owned();
                    let label = quality["label"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| id.clone());
                    let size = quality["size_bytes"]
                        .as_str()
                        .and_then(|size| size.parse::<u64>().ok())
                        .map(format::bytes)
                        .unwrap_or_default();
                    Some((
                        id,
                        if label == "original" {
                            tr("原画（原始文件）")
                        } else {
                            label
                        },
                        size,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Downloads with an upper quality bound instead of the default tier.
pub async fn download_quality(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    if let Err(reason) = ctx.gate(Operation::QualityDownload) {
        return ctx.toast(reason, "info");
    }
    let choices: Vec<(String, String, String)> = ["original", "2160p", "1080p", "720p", "480p"]
        .iter()
        .map(|quality| {
            let label = if *quality == "original" {
                tr("原画（原始文件）")
            } else {
                tx("不高于 {0}", &[quality])
            };
            ((*quality).to_owned(), label, String::new())
        })
        .collect();
    let current = ctx.core.borrow().settings["download"]["default_quality"]
        .as_str()
        .unwrap_or("original")
        .to_owned();
    let body = tr("转码画质由服务端提供；没有对应档位的文件会使用不高于所选档的最高画质。原画与转码不会静默互换。");
    let Some(quality) = ctx
        .choose(&tr("下载指定画质"), &body, &tr("下载"), choices, &current)
        .await
    else {
        return;
    };
    let Some(destination) = crate::actions::download_directory(ctx).await else {
        return;
    };
    let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
    let options = TransferOptions {
        quality_id: None,
        quality_cap: Some(quality),
        clip: None,
    };
    let result = ctx
        .call(move |service| async move {
            nekopak_core::validate_destination(&destination)?;
            service
                .transfer_start_batch_options(
                    &ids,
                    &destination,
                    &new_key(),
                    Some(&options),
                    &request(),
                )
                .await
        })
        .await;
    match result {
        Ok(batch) => {
            if !batch.task_ids.is_empty() {
                ctx.toast_action(
                    tx(
                        "已加入下载队列 {0} 项",
                        &[&batch.task_ids.len().to_string()],
                    ),
                    "success",
                    tr("查看"),
                    |ctx| ctx.navigate(Route::Transfers { tab: "all".into() }),
                );
            }
            if let Some(skipped) = batch.skipped.first() {
                ctx.toast(crate::i18n::message(&skipped.reason), "warning");
            }
            crate::transfers::request_reload(ctx);
        }
        Err(error) => ctx.toast_error(&error),
    }
}

/// Parses `1:02:03.5`, `02:03`, `75` (seconds) into milliseconds.
pub fn parse_time(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    let mut total = 0f64;
    for (index, part) in parts.iter().enumerate() {
        let value: f64 = part.parse().ok()?;
        let last = index == parts.len() - 1;
        if value < 0.0
            || !value.is_finite()
            || (!last && value.fract() != 0.0)
            || (index > 0 && value >= 60.0)
        {
            return None;
        }
        total = total * 60.0 + value;
    }
    Some((total * 1000.0).round() as u64)
}

/// Time-range download: probe, pick a range, show keyframe alignment, submit.
pub async fn clip_dialog(ctx: &Ctx, entry: RemoteEntry) {
    if !entry.id.starts_with("local:") {
        if let Err(reason) = ctx.gate(Operation::ClipDownload) {
            return ctx.toast(reason, "info");
        }
    }
    ctx.toast(tr("正在读取视频信息…"), "info");
    let id = entry.id.clone();
    let probe = ctx
        .call(move |service| async move {
            service
                .media_probe(
                    &id,
                    &nekopak_provider::RequestContext::new(std::time::Duration::from_secs(90)),
                )
                .await
        })
        .await;
    let probe = match probe {
        Ok(probe) => probe,
        Err(error) => return ctx.toast_error(&error),
    };
    let duration = probe["duration_ms"].as_u64().unwrap_or(0);
    let qualities = quality_choices(&probe);
    let mut quality = qualities
        .first()
        .map(|(id, ..)| id.clone())
        .unwrap_or_else(|| "original".to_owned());
    let quality_label = |id: &str| {
        qualities
            .iter()
            .find(|(candidate, ..)| candidate == id)
            .map(|(_, label, _)| label.clone())
            .unwrap_or_else(|| id.to_owned())
    };
    let mut fields = vec![
        Field::note(
            "info",
            format!(
                "{} · {}{}",
                format::media_time(duration),
                probe["container"]
                    .as_str()
                    .unwrap_or_default()
                    .to_uppercase(),
                match (probe["width"].as_u64(), probe["height"].as_u64()) {
                    (Some(width), Some(height)) => format!(" · {width}×{height}"),
                    _ => String::new(),
                }
            ),
            "info",
        ),
        Field::text("start", tr("开始时间"), "00:00")
            .placeholder("mm:ss")
            .focus(),
        Field::text("end", tr("结束时间"), format::media_time(duration)).placeholder("mm:ss"),
        Field::select("quality", tr("画质"), quality_label(&quality)).disabled(qualities.len() < 2),
        Field::note(
            "align",
            tr("片段从所选时间之前最近的关键帧开始，不重新编码。"),
            "info",
        ),
    ];
    for warning in probe["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(3)
    {
        fields.push(Field::note("warning", tr(warning), "warn"));
    }
    let mut form = ctx.form(
        &tr("下载指定段落"),
        &entry.name,
        &tr("下载片段"),
        &tr("检查对齐"),
        fields,
    );
    let range = |form: &crate::overlay::Form| -> Result<(u64, u64), String> {
        let start =
            parse_time(&form.value("start")).ok_or_else(|| tr("开始时间格式应为 时:分:秒"))?;
        let end = parse_time(&form.value("end")).ok_or_else(|| tr("结束时间格式应为 时:分:秒"))?;
        if end <= start {
            return Err(tr("结束时间必须晚于开始时间"));
        }
        if duration > 0 && end > duration {
            return Err(tx(
                "结束时间不能超过视频时长 {0}",
                &[&format::media_time(duration)],
            ));
        }
        Ok((start, end))
    };
    let chosen = loop {
        match form.next().await {
            FormEvent::Select(_, x, y) => {
                let items = qualities.iter().map(|(id, label, size)| {
                    crate::overlay::MenuItem::new(
                        id,
                        if size.is_empty() {
                            label.clone()
                        } else {
                            format!("{label} · {size}")
                        },
                    )
                    .checked(*id == quality)
                });
                if let Some(choice) = ctx.menu(x, y, "", items.collect()).await {
                    quality = choice;
                    let label = quality_label(&quality);
                    form.set("quality", |field| field.value = label);
                }
            }
            FormEvent::Alt => match range(&form) {
                Ok((start, end)) => {
                    form.set_busy(true);
                    let (id, wanted) = (entry.id.clone(), quality.clone());
                    let aligned = ctx
                        .call(move |service| async move {
                            service
                                .media_align(&id, &wanted, start, end, &request())
                                .await
                        })
                        .await;
                    form.set_busy(false);
                    match aligned {
                        Ok(value) => {
                            let from = value["aligned_start_ms"]
                                .as_u64()
                                .or(value["start_ms"].as_u64())
                                .unwrap_or(start);
                            let to = value["aligned_end_ms"]
                                .as_u64()
                                .or(value["end_ms"].as_u64())
                                .unwrap_or(end);
                            let size = value["estimated_bytes"]
                                .as_str()
                                .and_then(|size| size.parse::<u64>().ok());
                            let text = format!(
                                "{} {} – {}{}",
                                tr("实际片段："),
                                format::media_time(from),
                                format::media_time(to),
                                size.map(|size| format!(
                                    " · {}",
                                    tx("约 {0}", &[&format::bytes(size)])
                                ))
                                .unwrap_or_default()
                            );
                            form.set_error("");
                            form.set("align", |field| {
                                field.label = text;
                                field.tone = "ok";
                            });
                        }
                        Err(error) => form.set_error(error_text(&error)),
                    }
                }
                Err(message) => form.set_error(message),
            },
            FormEvent::Submit => match range(&form) {
                Ok(range) => break Some(range),
                Err(message) => form.set_error(message),
            },
            FormEvent::Cancel => break None,
            _ => {}
        }
    };
    form.close();
    let Some((start_ms, end_ms)) = chosen else {
        return;
    };
    let Some(destination) = crate::actions::download_directory(ctx).await else {
        return;
    };
    let options = TransferOptions {
        quality_id: Some(quality),
        quality_cap: None,
        clip: Some(nekopak_core::ClipOptions {
            start_ms,
            end_ms,
            align: "previous_keyframe".into(),
        }),
    };
    let id = entry.id.clone();
    let result = ctx
        .call(move |service| async move {
            nekopak_core::validate_destination(&destination)?;
            service
                .media_start(&id, &destination, &options, Some(&new_key()), &request())
                .await
        })
        .await;
    match result {
        Ok(_) => {
            ctx.toast_action(tr("已加入片段下载"), "success", tr("查看"), |ctx| {
                ctx.navigate(Route::Transfers { tab: "all".into() })
            });
            crate::transfers::request_reload(ctx);
        }
        Err(error) => ctx.toast_error(&error),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_time;

    #[test]
    fn clip_times_accept_common_notations() {
        assert_eq!(parse_time("75"), Some(75_000));
        assert_eq!(parse_time("01:15"), Some(75_000));
        assert_eq!(parse_time("1:02:03.5"), Some(3_723_500));
        assert_eq!(parse_time(" 0:00 "), Some(0));
        assert_eq!(parse_time("1:75"), None);
        assert_eq!(parse_time("1.5:00"), None);
        assert_eq!(parse_time("a:b"), None);
        assert_eq!(parse_time("1::2"), None);
        assert_eq!(parse_time("-3"), None);
    }
}
