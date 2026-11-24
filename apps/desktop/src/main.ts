import "./styles.css";
import { mount } from "svelte";
import App from "./App.svelte";

// Block the browser's own context menu & reload shortcuts – this is an app.
window.addEventListener("contextmenu", (e) => {
  const el = e.target as HTMLElement;
  if (!el.closest("input, textarea")) e.preventDefault();
});
window.addEventListener("keydown", (e) => {
  if (e.key === "F5" || (e.ctrlKey && e.key.toLowerCase() === "r")) e.preventDefault();
});

export default mount(App, { target: document.getElementById("app")! });
