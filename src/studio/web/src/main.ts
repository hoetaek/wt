import { h, render } from "preact";
import "./style.css";

function App() {
  return h("main", { class: "studio-shell" }, [
    h("header", { class: "studio-header" }, [
      h("p", { class: "eyebrow" }, "wt studio"),
      h("h1", {}, "Studio (stub)"),
      h("p", { class: "summary" }, "TaskDocument authoring surface placeholder")
    ]),
    h("section", { class: "panel", "aria-label": "TaskDocument list" }, [
      h("div", { class: "panel-header" }, [
        h("h2", {}, "TaskDocuments"),
        h("span", { class: "status" }, "T1 skeleton")
      ]),
      h("div", { class: "empty-state" }, [
        h("strong", {}, "Editor routes land in the next stack task."),
        h(
          "p",
          {},
          "This page verifies the Vite, Preact, TypeScript, embedded asset, and authenticated server foundation."
        )
      ])
    ])
  ]);
}

render(h(App, {}), document.getElementById("app") as HTMLElement);
