import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";

const container = document.getElementById("root");
if (!container) {
  throw new Error("#root is missing from index.html");
}

// Note: no StrictMode. Its development double-invoke is a useful check for
// pure effects, but a TerminalPane effect owns a real PTY and a WebGL context,
// so the second invocation would open a second shell inside one pane. Pane
// lifecycle is kept explicit rather than idempotent.
createRoot(container).render(<App />);
