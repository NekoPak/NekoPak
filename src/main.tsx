import { render } from "preact";
import { App } from "./app/App";
import { ensureLocale, language, setLanguage } from "./lib/i18n";
import "./styles/schemes.css";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/shell.css";
import "./styles/components.css";
import "./styles/pages.css";
import "./styles/features.css";
import "./styles/mobile.css";
import "./styles/phone.css";

const root = document.getElementById("app");
setLanguage(language.get().preference);
// First paint waits for the one catalog this user needs (none for zh-CN).
void ensureLocale().then(() => { if (root) render(<App />, root); });
