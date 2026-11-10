import { tr } from "../lib/i18n";
import { useEffect } from "preact/hooks";
import { useStore } from "../lib/store";
import { acceptDisclaimer, app, isMobileHost } from "../state/app";
import { navigate } from "../state/route";
import { LogoMark } from "../components/Brand";
import { Btn } from "../components/ui";
import { WindowControls } from "../components/Frame";
import { AuthPanel } from "./Auth";
import { useMobileLayout } from "../lib/mobile";

/**
 * First launch / signed out: brand on the frame colour, the sign-in island
 * floating over it. Signing in goes straight to the cloud drive.
 */
export function Onboarding({ onClose }: { onClose: () => void }) {
  const auth = useStore(app, (x) => x.auth?.state);
  const mobile = useStore(app, isMobileHost);
  const phone = useMobileLayout();

  useEffect(() => {
    if (auth === "signed_in" || auth === "demo") {
      acceptDisclaimer();
      onClose();
      navigate({ page: "files", folder: "root" });
    }
  }, [auth]);

  return (
    <div class={phone ? "phone-login" : "onboarding"} role="dialog" aria-modal="true" aria-labelledby="ob-title">
      <div class="ob-caption" data-tauri-drag-region={mobile ? undefined : true}><span class="pf-drag" data-tauri-drag-region={mobile ? undefined : true} /><WindowControls /></div>
      <div class="ob-brand">
        <LogoMark size={72} />
        <p class="ob-name">NekoPak</p>
        <p class="ob-tagline">{tr(mobile ? "轻快的 PikPak 客户端" : "轻快的 PikPak 桌面客户端")}</p>
      </div>
      <div class="ob-island">
        <div class="ob-body">
          <h1 id="ob-title">{tr("登录 PikPak")}</h1>
          <AuthPanel compact />
          <div class="ob-actions spread">
            <Btn tone="ghost" label={tr("先看看")} onClick={() => { acceptDisclaimer(); onClose(); }} />
          </div>
        </div>
      </div>
    </div>
  );
}
