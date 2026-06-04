import { render } from "preact";
import "./style.css";

function App() {
  return (
    <main class="studio-reset" aria-label="wt studio">
      <section class="studio-reset__panel" aria-label="Studio reset state">
        <p class="studio-reset__eyebrow">wt studio</p>
        <h1>TaskDocument authoring</h1>
        <p>UI cleared. Ready for a fresh build.</p>
      </section>
    </main>
  );
}

render(<App />, document.getElementById("app") as HTMLElement);
