# Third-Party Notices / 第三方开源致谢（模板）

**维护说明：** 本模板只是计划参考清单，不代表已实际复制代码。发布前将每条标记为 `CODE_ADAPTED`、`DESIGN_REFERENCE`、`EXTERNAL_TOOL_OPTIONAL` 或 `NOT_USED`，删除不准确的内容，填写精确版本、SHA 与许可全文/链接。

NekoPak is an independently developed community project and is not affiliated with or endorsed by PikPak.

| Name | URL | Declared license (verify before release) | Intended relationship | Actual SHA / paths |
|---|---|---|---|---|
| Piko | https://github.com/NihilDigit/piko | AGPL-3.0-or-later | DESIGN_REFERENCE or FORK | TODO |
| pikpaktui | https://github.com/Bengerthelorf/pikpaktui | Apache-2.0 | CODE_ADAPTED (only after audit) | TODO |
| 52funny/pikpakcli | https://github.com/52funny/pikpakcli | MIT | DESIGN_REFERENCE | TODO |
| PikPak Enhancement Master | https://github.com/digbug82/PikPak_Enhancement_Master | AGPL-3.0 | DESIGN_REFERENCE | TODO |
| pikpak-kotlin | https://github.com/NihilDigit/pikpak-kotlin | MIT | DESIGN_REFERENCE | TODO |
| MediaMP | https://github.com/open-ani/mediamp | Apache-2.0 | DESIGN_REFERENCE | TODO |
| Rclone | https://github.com/rclone/rclone | MIT | EXTERNAL_TOOL_OPTIONAL | TODO |
| WinFsp | https://github.com/winfsp/winfsp | GPLv3 + special FLOSS exception | EXTERNAL_TOOL_OPTIONAL | TODO |
| Tauri | https://github.com/tauri-apps/tauri | Check component licenses | RUNTIME_DEPENDENCY | TODO |
| Windows Cloud Files API | https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot | Microsoft platform API | PLATFORM_API | N/A |
| PikPak official CLI | https://mypikpak.com/en-US/cli | Not established as a redistributable OSS CLI | EXTERNAL_TOOL_OPTIONAL | TODO |

## Attribution details / 溯源记录

### Example: pikpaktui download logic (unconfirmed)

- Original path: `src/pikpak/download.rs`
- Upstream commit: `TODO`
- NekoPak path: `TODO`
- Whether copied/adapted: `TODO`
- Copyright/license notices preserved: `TODO`
- Modifications performed: `TODO`

**Do not publish this example as a factual use record until code has been incorporated.**

## Publication checklist

- [ ] LICENSE agrees with all **actually incorporated** code
- [ ] All required upstream licenses/notices included in distributable
- [ ] Source file copyright headers preserved where necessary
- [ ] Attribution accurately distinguishes inspirations from actual source re-use
- [ ] No official PikPak logos/trademarks imply endorsement
- [ ] Fork history and AGPL obligations addressed if using Piko code
- [ ] Optional bundled artifacts redistribution licenses verified
