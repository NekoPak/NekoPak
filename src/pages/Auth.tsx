import { tr, tx } from "../lib/i18n";
import { useEffect, useRef, useState } from "preact/hooks";
import type { SavedAccount, SignInChallenge } from "../lib/contract-proposals";
import { api, errorOf, openExternal } from "../lib/api";
import { useStore } from "../lib/store";
import { app, canSignIn, logout, recoverAuth, removeAccount, signIn, switchAccount, verifySignIn } from "../state/app";
import { openDialog, toast } from "../state/ui";
import { Icon } from "../components/Icon";
import { Btn, ErrorPanel } from "../components/ui";
import { diagnosticsText } from "../lib/errors";
import { copyText } from "../lib/api";
import { confirmDialog } from "../components/dialogs";
import { sessionGeneration } from "../state/session";

function messageOf(e: unknown): string {
  const err = errorOf(e);
  if (err.code === "TRANSIENT_NETWORK") return tr("无法连接 PikPak，请检查网络后重试");
  return err.message;
}

/** Account card when signed in, otherwise the sign-in form. */
export function AuthPanel({ compact }: { compact?: boolean }) {
  const s = useStore(app, (x) => x);
  if (s.auth?.state === "demo") return <p class="inline-note">{tr("离线界面预览：未连接 PikPak，不使用真实账号或密码。")}</p>;
  if (s.runtime === "tauri" && s.authError && s.authErrorSource !== "sign_in" && s.authError.details?.reason !== "invalid_credentials") {
    return <div class="signin">
      <ErrorPanel error={s.authError} context="auth_status" />
      <Btn icon="refresh" label={tr("重试连接")} busy={s.authBusy} onClick={() => void recoverAuth()} />
      {s.app?.feature_flags.auth_logout ? <Btn label={tr("清除本地会话")} onClick={() => void logout().catch(e => toast(errorOf(e).message, "error"))} /> : null}
    </div>;
  }
  if (s.auth?.state === "signed_in") {
    const name = s.auth.account?.display_name ?? tr("已登录");
    return (
      <div class="acct-card">
        <span class="avatar lg state-signed_in" aria-hidden="true">{(name.trim()[0] ?? "N").toUpperCase()}</span>
        <div class="acct-card-text">
          <strong dir="auto">{name}</strong>
          <span class="dim">{s.auth.account?.tier === "premium" ? tr("Premium 会员") : s.auth.account?.tier === "free" ? tr("免费账号") : ""}</span>
        </div>
        {!compact ? <Btn icon="logout" label={tr("退出登录")} onClick={() => void logout().then(() => toast(tr("已退出登录"), "success")).catch(e => toast(errorOf(e).message, "error"))} /> : null}
      </div>
    );
  }
  return <SignInForm />;
}

function SignInForm() {
  const busy = useStore(app, (x) => x.authBusy);
  const expired = useStore(app, (x) => x.auth?.state === "expired");
  const demo = useStore(app, (x) => x.runtime === "mock");
  const available = useStore(app, canSignIn);
  const lastError = useStore(app, s => s.authError);
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  const [show, setShow] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [challenge, setChallenge] = useState<SignInChallenge | null>(null);
  const [code, setCode] = useState("");
  const [wait, setWait] = useState(0);
  const passRef = useRef<HTMLInputElement>(null);
  const codeRef = useRef<HTMLInputElement>(null);
  const [remember,setRemember]=useState(false);
  const [restore,setRestore]=useState(true);
  const [preferencesReady,setPreferencesReady]=useState(false);
  const touchedUser=useRef(false);
  useEffect(()=>{
    if(demo)return;
    let alive=true;
    void api.authPreferencesGet().then(value=>{if(!alive)return;setRemember(value.remember_account);setRestore(value.restore_session);if(!touchedUser.current&&value.username)setUser(value.username);setPreferencesReady(true);},()=>{if(alive)setPreferencesReady(false);});
    return()=>{alive=false;};
  },[demo]);

  useEffect(() => {
    if (wait <= 0) return;
    const t = setTimeout(() => setWait(wait - 1), 1000);
    return () => clearTimeout(t);
  }, [wait]);
  useEffect(() => {
    if (challenge?.kind === "verify_code") codeRef.current?.focus();
  }, [challenge?.challenge_id]);

  const enter = (ch: SignInChallenge | null) => {
    setChallenge(ch);
    setCode("");
    if (ch?.kind === "verify_code") setWait(ch.resend_after_s ?? 0);
  };

  const submit = async () => {
    if (!available || busy) return;
    if (!user.trim()) return setErr(tr("请输入账号"));
    if (!pass) {
      passRef.current?.focus();
      return setErr(tr("请输入密码"));
    }
    setErr(null);
    try {
      if(!demo&&preferencesReady)await api.authPreferencesUpdate({remember_account:remember,restore_session:restore,...(remember?{username:user.trim()}: {})});
      const ch = await signIn(user.trim(), pass);
      enter(ch);
    } catch (e) {
      setErr(messageOf(e));
      if (errorOf(e).details?.reason === "invalid_credentials") {
        passRef.current?.focus();
      }
    } finally {
      setPass("");
      if (passRef.current) passRef.current.value = "";
    }
  };

  const verify = async () => {
    if (!challenge) return;
    if (challenge.kind === "captcha" && challenge.requires_password && !pass) return setErr(tr("请重新输入密码以完成登录"));
    if (challenge.kind === "verify_code" && code.trim().length < challenge.code_length) return setErr(tx("请输入 {0} 位验证码", challenge.code_length));
    setErr(null);
    try {
      enter(await verifySignIn(challenge.challenge_id, challenge.kind === "verify_code" ? code.trim() : "", challenge.kind === "captcha" && challenge.requires_password ? pass : undefined));
    } catch (e) {
      setErr(messageOf(e));
      setCode("");
      codeRef.current?.focus();
    } finally {
      setPass("");
      if (passRef.current) passRef.current.value = "";
    }
  };

  const resend = async () => {
    if (!challenge) return;
    try {
      const r = await api.authResend(challenge.challenge_id);
      setWait(r.resend_after_s);
      toast(tr("验证码已重新发送"), "success");
    } catch (e) {
      setErr(messageOf(e));
    }
  };

  const back = (
    <button type="button" class="link-btn" disabled={busy} onClick={() => { enter(null); setPass(""); setErr(null); }}>{tr("返回")}</button>
  );

  if (challenge?.kind === "captcha") {
    return (
      <form class="signin" onSubmit={(e) => { e.preventDefault(); if (!busy) void verify(); }} noValidate>
        <p class="signin-note">{tr("PikPak 要求先完成一次安全验证。")}</p>
        <Btn tone="tonal" icon="external" label={tr("打开验证页面")} onClick={() => (demo ? toast(tr("演示模式不会打开验证页面"), "info") : void openExternal(challenge.url).then(result => {
          if (result !== "opened") toast(result === "copied" ? tr("验证链接已复制，请在浏览器打开。") : tr("无法打开验证页面，请重试。"), result === "copied" ? "info" : "error");
        }))} />
        {challenge.requires_password ? <label class="signin-field"><span>{tr("重新输入密码")}</span><input ref={passRef} class="input" type="password" name="password" autocomplete="current-password" value={pass} disabled={busy} onInput={e => setPass((e.target as HTMLInputElement).value)} /><span class="dim">{tr("首次提交的密码已清除，完成安全验证后请再次输入。")}</span></label> : null}
        {err ? <p class="field-error" role="alert">{err}</p> : null}
        {err && lastError ? <button type="button" class="link-btn" onClick={() => void copyText(diagnosticsText(lastError, "auth_verify")).then(ok => toast(ok ? tr("诊断信息已复制") : tr("复制失败"), ok ? "info" : "error"))}>{tr("复制诊断信息")}</button> : null}
        <Btn type="submit" tone="primary" label={tr("已完成验证，继续登录")} busy={busy} class="signin-go" />
        <div class="signin-row">{back}</div>
        {demo ? <p class="dim signin-demo"><Icon name="flask" size={14} />{tr("演示：直接点“继续登录”。")}</p> : null}
      </form>
    );
  }

  if (challenge?.kind === "verify_code") {
    return (
      <form class="signin" onSubmit={(e) => { e.preventDefault(); if (!busy) void verify(); }} noValidate>
        <p class="signin-note">{tr("验证码已发送到")}{challenge.channel === "sms" ? tr("手机") : tr("邮箱")} <strong class="tabular">{challenge.target}</strong></p>
        <label class="signin-field">
          <span>{tr("验证码")}</span>
          <input ref={codeRef} class="input code-input tabular" inputMode="numeric" autocomplete="one-time-code" maxLength={challenge.code_length}
            value={code} aria-invalid={err ? "true" : undefined}
            onInput={(e) => { setCode((e.target as HTMLInputElement).value.replace(/\D/g, "")); setErr(null); }} />
        </label>
        {err ? <p class="field-error" role="alert">{err}</p> : null}
        <Btn type="submit" tone="primary" label={tr("验证并登录")} busy={busy} class="signin-go" />
        <div class="signin-row">
          {back}
          <button type="button" class="link-btn" disabled={wait > 0} onClick={() => void resend()}>{wait > 0 ? tx("{0} 秒后可重新发送", wait) : tr("重新发送")}</button>
        </div>
        {demo ? <p class="dim signin-demo"><Icon name="flask" size={14} />{tr("演示：验证码是 123456。")}</p> : null}
      </form>
    );
  }

  return (
    <form class="signin" onSubmit={(e) => { e.preventDefault(); if (!busy) void submit(); }} noValidate>
      {expired ? <p class="inline-note warn"><Icon name="alert" size={16} />{tr("登录已过期，请重新登录。")}</p> : null}
      {!available ? <p class="inline-note">{tr("当前版本尚不能连接账号，可以先浏览界面。")}</p> : null}
      <label class="signin-field">
        <span>{tr("账号")}</span>
        <input class="input" type="text" name="username" autocomplete="username" placeholder={demo ? tr("邮箱或手机号") : tr("邮箱")} value={user} spellcheck={false} disabled={!available || busy}
          onInput={(e) => { touchedUser.current=true;setUser((e.target as HTMLInputElement).value); setErr(null); }} data-autofocus="" />
      </label>
      <label class="signin-field">
        <span>{tr("密码")}</span>
        <span class="pass-wrap">
          <input ref={passRef} class="input" name="password" type={show ? "text" : "password"} autocomplete="current-password" value={pass} disabled={!available || busy}
            aria-invalid={err ? "true" : undefined}
            onInput={(e) => { setPass((e.target as HTMLInputElement).value); setErr(null); }} />
          <button type="button" class="pass-eye" aria-label={show ? tr("隐藏密码") : tr("显示密码")} title={show ? tr("隐藏密码") : tr("显示密码")} onClick={() => setShow(!show)}>
            <Icon name={show ? "visibilityOff" : "visibility"} size={20} />
          </button>
        </span>
      </label>
      {err ? <p class="field-error" role="alert">{err}</p> : null}
      {err && lastError ? <button type="button" class="link-btn" onClick={() => void copyText(diagnosticsText(lastError, "auth_sign_in")).then(ok => toast(ok ? tr("诊断信息已复制") : tr("复制失败"), ok ? "info" : "error"))}>{tr("复制诊断信息")}</button> : null}
      {!demo&&preferencesReady?<div class="phone-auth-preferences"><label><input type="checkbox" checked={remember} onChange={e=>setRemember((e.target as HTMLInputElement).checked)}/><span>{tr("记住账号")}</span></label><label><input type="checkbox" checked={restore} onChange={e=>setRestore((e.target as HTMLInputElement).checked)}/><span>{tr("保持登录")}</span></label></div>:null}
      <Btn type="submit" tone="primary" label={tr("登录")} busy={busy} disabled={!available} reason={tr("当前版本尚未配置可用的账号登录")} class="signin-go" />
      {demo ? (
        <p class="dim signin-demo"><Icon name="flask" size={14} />{tr("演示：任意账号和密码都能登录。密码填 wrong 会提示错误，填 captcha 会要求安全验证；手机号会要求验证码。")}</p>
      ) : null}
    </form>
  );
}

export function SavedAccounts() {
  const state = useStore(app, s => s);
  const enabled = state.app?.feature_flags.accounts === true;
  const [accounts, setAccounts] = useState<SavedAccount[]>();
  const [error, setError] = useState<ReturnType<typeof errorOf>>();
  const [working, setWorking] = useState<string>();
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    setError(undefined);
    void api.accountsList().then(result => { if (alive) setAccounts(result); }, e => { if (alive) setError(errorOf(e)); });
    return () => { alive = false; };
  }, [enabled, state.sessionEpoch, state.auth?.account_id, revision]);
  if (!enabled) return null;
  const change = async (account: SavedAccount) => {
    setWorking(account.id); setError(undefined);
    try { await switchAccount(account.id); toast(tr("已切换账号"), "success"); }
    catch (e) { setError(errorOf(e)); }
    finally { setWorking(undefined); setRevision(n => n + 1); }
  };
  const remove = async (account: SavedAccount) => {
    const generation = sessionGeneration();
    if (!await confirmDialog({ title: tr("移除此设备保存的账号？"), body: <p>{tr("只清除此设备的登录会话。云盘文件和其他设备的登录保持不变。")}</p>, confirmLabel: tr("移除账号"), danger: true }) || generation !== sessionGeneration()) return;
    setWorking(account.id); setError(undefined);
    try { await removeAccount(account.id, account.active); setRevision(n => n + 1); }
    catch (e) { setError(errorOf(e)); }
    finally { setWorking(undefined); }
  };
  return <div class="saved-accounts">
    <div class="spread"><h3>{tr("已保存的账号")}</h3><Btn label={tr("添加账号")} icon="plus" disabled={!canSignIn(state) || !!working || state.authBusy} reason={tr("当前版本尚未配置可用的账号登录")} onClick={() => void openDialog({ title: tr("添加账号"), width: 480, render: (close) => <AddAccountBody close={close}/> })}/></div>
    {error ? <ErrorPanel error={error} onRetry={() => setRevision(n => n + 1)}/> : null}
    {accounts?.length === 0 ? <p class="muted">{tr("还没有已保存的账号")}</p> : null}
    <ul class="account-list">{accounts?.map(account => <li key={account.id}>
      <Icon name="user"/><div class="account-info"><strong dir="auto">{account.display_name}</strong><span class="dim">{account.active ? tr("当前账号") : account.state === "expired" ? tr("登录已过期") : tr("可切换")}</span></div>
      {!account.active ? <Btn label={tr("切换账号")} busy={working === account.id} disabled={!!working || state.authBusy} onClick={() => void change(account)}/> : null}
      <Btn label={tr("移除账号")} tone="quiet" busy={working === account.id} disabled={!!working || state.authBusy} onClick={() => void remove(account)}/>
    </li>)}</ul>
  </div>;
}

function AddAccountBody({ close }: { close: (value?: unknown) => void }) {
  const auth = useStore(app, s => s.auth);
  const [initial] = useState(() => ({ signedIn: auth?.state === "signed_in", id: auth?.account_id }));
  useEffect(() => {
    if (auth?.state === "signed_in" && (!initial.signedIn || auth.account_id !== initial.id)) close(true);
  }, [auth?.state, auth?.account_id]);
  return <><p class="muted pad-bottom">{tr("新账号登录成功后自动切换；当前账号会保留在此设备。")}</p><SignInForm/></>;
}
