import { render } from "preact";
import { App } from "./app/App";
import "./styles/schemes.css";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/shell.css";
import "./styles/components.css";
import "./styles/pages.css";

const root = document.getElementById("app");
if (root) render(<App />, root);
