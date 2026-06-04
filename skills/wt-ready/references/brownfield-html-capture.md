# Brownfield HTML capture for the wireframe gate

A fast way to build the **artifact-specific (UI/web) wireframe** in Gate 4 when the
work is a **brownfield change to an existing web page**: instead of hand-drawing
screens, capture the real rendered page with Chrome, edit only the regions that
change, and save a self-contained single file.

## When to use (and when not)

- **Use** when: the work modifies an existing, reachable web surface; the page is
  largely server-rendered or its rendered DOM is stable at capture time. The real
  page becomes the **locked context** and doubles as the Static Model "current
  structure" evidence for `03-Architect/05-design.md`.
- **Avoid / fall back** when: heavily hashed/minified class names make region edits
  harder than writing fresh; the page is a JS-heavy SPA whose meaningful state only
  exists after interaction; assets are auth-gated or on cross-origin CDNs that block
  `fetch`. Fall back to: screenshot + DOM outline + a hand text-first wireframe.
- **Not for greenfield.** Copying a reference site's full HTML anchors you to its
  design. Brownfield is the sweet spot because you must preserve the existing screen
  anyway.

**Text-first still comes first.** This is a medium-specific pass (Gate 4 step 3),
never a replacement for the text-first wireframe (step 2). Group requirements and
walk the journey in text before capturing.

## Prerequisites

Any way to run JS in the page and read back a string works. The reference flow uses
the Chrome DevTools MCP: `navigate_page` to the target, `evaluate_script` to capture
and edit, `take_screenshot` to record, `emulate` to prove offline.

## Recipe

1. **Capture baseline.** Open the page in Chrome (use the already-authenticated
   profile if the surface needs login). The full rendered DOM is
   `document.documentElement.outerHTML`. This baseline is the **locked context** —
   do not redraw it.
2. **Edit only the changed regions.** Inject your change into the DOM and mark it
   with a visible diff (dashed outline + a small badge), so a cold reader can see
   exactly what changes. Everything you do not touch stays as verified reality.
3. **Preserve assets in three steps** (only as far as you need):
   1. `<img src>` → inline as `data:` URI (same-origin `fetch` → `readAsDataURL`).
   2. `<link rel=stylesheet>` → fetch the CSS text, inline as `<style>`.
   3. `url(...)` inside CSS (fonts, background images) → inline as `data:` URI.
      Many server-rendered pages use system fonts (no webfont download) and the
      remaining `url()`s are unused decorations, so this step is often a no-op.
4. **Save** the edited `outerHTML` as one `.html` file.
5. **Verify self-containment.** Open the file via `file://`, set Chrome network to
   **Offline**, and reload. If it renders identically, the single file is
   server-independent.

## Locked context vs variation point (the Gate 4 contract)

| | meaning | in the wireframe |
|---|---|---|
| **Locked context** | untouched real markup | not a variation point — verified reality |
| **Variation point** | the regions you replaced with mock data | name axis + range (e.g. list 0..N, empty state, overflow, on/off) |

Capturing real HTML does not break "cheap, throwaway instance": you only throw away
the mock in the changed regions; the baseline is real and never redrawn.

## Verified inliner (copy-paste)

Run on the captured page after injecting your edits. Same-origin assets only; on any
fetch failure it falls back to an absolute URL (still works while the server is up).

```js
// 1) inline <img> as data URIs
async () => {
  const toData = async (u) => {
    const r = await fetch(u, { credentials: 'include' });
    if (!r.ok) return null;
    const b = await r.blob();
    return await new Promise((res, rej) => {
      const fr = new FileReader(); fr.onload = () => res(fr.result); fr.onerror = rej;
      fr.readAsDataURL(b);
    });
  };
  for (const img of document.querySelectorAll('img')) {
    const d = await toData(img.src).catch(() => null);
    if (d) { img.setAttribute('src', d); img.removeAttribute('srcset'); }
  }
  return 'img inlined';
}
```

```js
// 2+3) inline stylesheets, and the url() assets inside them
async () => {
  const abs = (base, u) => { try { return new URL(u, base).href; } catch { return null; } };
  const toData = async (u) => {
    try {
      const r = await fetch(u, { credentials: 'include' }); if (!r.ok) return null;
      const b = await r.blob();
      return await new Promise((res, rej) => {
        const fr = new FileReader(); fr.onload = () => res(fr.result); fr.onerror = rej;
        fr.readAsDataURL(b);
      });
    } catch { return null; }
  };
  for (const link of [...document.querySelectorAll('link[rel=stylesheet]')]) {
    const cssUrl = link.href; let css;
    try { const r = await fetch(cssUrl, { credentials: 'include' }); if (!r.ok) continue; css = await r.text(); }
    catch { continue; }
    const refs = new Set(); let m;
    const re = /url\(\s*(['"]?)([^'")]+)\1\s*\)/g;
    while ((m = re.exec(css))) { const u = m[2].trim(); if (u && !u.startsWith('data:') && !u.startsWith('#')) refs.add(u); }
    for (const ref of refs) {
      const a = abs(cssUrl, ref); if (!a) continue;
      const d = await toData(a);
      css = css.split(ref).join(d || a); // data URI if possible, else absolute URL
    }
    const s = document.createElement('style'); s.setAttribute('data-from', cssUrl);
    s.textContent = css; link.replaceWith(s);
  }
  return 'css inlined';
}
```

```js
// 4) read the final self-contained document to save
() => '<!doctype html>\n' + document.documentElement.outerHTML
```

If you save via a tool that JSON-encodes the return value, decode it back to raw
text before writing the `.html` (e.g. `python3 -c "import json;open('out.html','w').write(json.load(open('raw.json')))"`).

## Stack limits (be honest in the wireframe)

Gate 4 validates one concrete instance. Record which of these your capture actually
covered, and which are deferred:

| axis | easy case | harder case → note / fall back |
|---|---|---|
| class names | readable utilities (Tailwind) | hashed/minified SPA classes → editing harder |
| fonts | system fonts (0 downloads) | CDN/webfonts → cross-origin `fetch` may fail |
| images | same-origin | auth-gated / external → fetch fails → absolute-URL fallback or placeholder |
| dynamic content | static capture is enough | JS-rendered widgets → frozen at capture-time state |

A page in dev mode may also capture debug overlays (e.g. a debug toolbar); offline
reload usually drops them because their JS does not load, which leaves a cleaner
wireframe — but strip them explicitly if they remain.
