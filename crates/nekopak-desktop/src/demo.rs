//! Demo controls (offline simulator only): switch the fixture scenario and
//! inject one-shot faults without restarting. Never available with a real
//! account.
use crate::{
    app::Ctx,
    i18n::{tr, tx},
    overlay::{Field, FormEvent, MenuItem},
};
use nekopak_domain::Operation;
use nekopak_provider_mock::{Fault, Scenario};

fn scenario_copy(scenario: Scenario) -> (&'static str, &'static str) {
    match scenario {
        Scenario::Free => ("免费账户", "免费账号的示例目录与配额"),
        Scenario::Premium => ("Premium", "Premium 账号的示例目录与配额"),
        Scenario::Unauthorized => ("未授权", "远端请求返回未登录"),
        Scenario::QuotaExhausted => ("限额耗尽", "下载、上传与云下载配额用尽，列表仍可查看"),
        Scenario::Offline => ("网络中断", "所有远端请求返回网络错误"),
        Scenario::LargeDirectory => ("目录超大", "根目录包含大量项目，用于检查分页与滚动"),
        Scenario::NameConflict => ("名称冲突", "重名、大小写与 Unicode 归一化冲突"),
        Scenario::DisconnectAt37 => ("下载到 37% 断线", "任务在 37% 失败，可手动重试"),
        Scenario::IntegrityFailure => ("完整性校验失败", "下载到 100% 后校验失败"),
        Scenario::DiskFull => ("磁盘已满", "下载开始后报告磁盘空间不足"),
    }
}

fn scenario_id(scenario: Scenario) -> String {
    serde_json::to_value(scenario)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

const FAULTS: [(&str, &str); 4] = [
    ("list-auth", "下一次列目录返回登录过期"),
    ("list-network", "下一次列目录返回服务不可用"),
    ("download-disk", "下一次下载报告磁盘已满"),
    ("download-integrity", "下一次下载校验失败"),
];

pub fn open(ctx: &Ctx) {
    let ctx = ctx.clone();
    ctx.clone().spawn(async move { panel(&ctx).await });
}

async fn panel(ctx: &Ctx) {
    let Some(provider) = ctx.demo.clone() else {
        return ctx.toast(tr("当前连接的是真实服务，演示控制不可用"), "info");
    };
    if ctx.dialog_open() {
        return;
    }
    let (label, description) = scenario_copy(provider.scenario());
    let fields = vec![
        Field::note(
            "about",
            tr("这些控制只存在于离线演示：切换示例数据或注入一次故障，用来查看各种状态下的界面。不会连接 PikPak，也不会写入任何文件。"),
            "info",
        ),
        Field::select("scenario", tr("场景"), tr(label)).detail(tr(description)),
        Field::select("fault", tr("注入一次故障"), tr("选择…"))
            .detail(tr("只影响下一次对应的请求，之后恢复正常。")),
    ];
    let mut form = ctx.form(&tr("演示场景与故障注入"), "", &tr("完成"), "", fields);
    loop {
        match form.next().await {
            FormEvent::Select(id, x, y) if id == "scenario" => {
                let current = provider.scenario();
                let items = Scenario::ALL
                    .iter()
                    .map(|scenario| {
                        MenuItem::new(&scenario_id(*scenario), tr(scenario_copy(*scenario).0))
                            .checked(*scenario == current)
                    })
                    .collect();
                let Some(choice) = ctx.menu(x, y, "", items).await else {
                    continue;
                };
                let Some(scenario) = Scenario::ALL
                    .iter()
                    .copied()
                    .find(|scenario| scenario_id(*scenario) == choice)
                else {
                    continue;
                };
                provider.set_scenario(scenario);
                let (label, description) = scenario_copy(scenario);
                form.set("scenario", |field| {
                    field.value = tr(label);
                    field.detail = tr(description);
                });
                crate::settings::resync(ctx).await;
                ctx.toast(tx("已切换到演示场景“{0}”", &[&tr(label)]), "success");
            }
            FormEvent::Select(id, x, y) if id == "fault" => {
                let items = FAULTS
                    .iter()
                    .map(|(id, label)| MenuItem::new(id, tr(label)))
                    .collect();
                let Some(choice) = ctx.menu(x, y, "", items).await else {
                    continue;
                };
                let (operation, fault) = match choice.as_str() {
                    "list-auth" => (Operation::List, Fault::Http(401)),
                    "list-network" => (Operation::List, Fault::Http(503)),
                    "download-disk" => (Operation::Download, Fault::DiskFull),
                    _ => (Operation::Download, Fault::Integrity),
                };
                provider.inject(operation, fault);
                let label = FAULTS
                    .iter()
                    .find(|(id, _)| *id == choice)
                    .map(|(_, label)| tr(label))
                    .unwrap_or_default();
                form.set("fault", |field| field.value = label);
                ctx.toast(tr("已注入一次故障"), "info");
            }
            FormEvent::Submit | FormEvent::Cancel => break,
            _ => {}
        }
    }
    form.close();
}
