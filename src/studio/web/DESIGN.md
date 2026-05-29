# Design System: wt studio

## 1. Visual Theme & Atmosphere
wt studio is a dense authoring console for task documents, configuration, prompts, and workflows. It should feel like a precise editorial operations tool: calm, compact, legible, and built for repeated use. It is not a landing page, marketing surface, or analytics dashboard.

- **Density:** 8/10. Show useful controls without creating visual noise.
- **Variance:** 4/10. Use restrained asymmetry through drawer state and content rhythm, not decorative layout tricks.
- **Motion:** 3/10. Motion confirms state changes; it never performs.
- **Personality:** Quiet, technical, durable. No playful embellishment, no cinematic drama.

The first screen must be the tool itself. The user should immediately understand: choose a surface, edit content, check save readiness, save.

## 2. Color Palette & Roles
- **Zinc Canvas** (#F4F4F5) - Primary app background.
- **Paper Surface** (#FFFFFF) - Main editor surfaces, list surfaces, and inputs in light mode.
- **Zinc Raised** (#FAFAFA) - Subtle secondary areas, collapsed details, and inactive segmented rails.
- **Off Black Ink** (#18181B) - Primary text, active neutral controls, and high-contrast selected tabs.
- **Zinc Muted** (#71717A) - Secondary labels, paths, helper copy, timestamps, and metadata.
- **Whisper Line** (rgba(24,24,27,0.10)) - Structural borders, input rings, and panel separation.
- **Signal Teal** (#0F766E) - The only product accent. Use for primary save actions, focus rings, and confirmed active intent.

Semantic colors are allowed only for state meaning:

- **Ready Green** - Successful validation or save-ready state.
- **Caution Amber** - Stale disk state or recoverable conflict.
- **Blocking Rose** - Validation that prevents saving.
- **Neutral Zinc** - Idle, unchanged, loading, or explanatory states.

Do not let semantic colors become decorative accents. Purple, neon blue, and saturated gradients are banned.

## 3. Typography Rules
- **Display and UI:** Geist Variable. Hierarchy comes from weight, spacing, and placement, not oversized text.
- **Code, paths, IDs, timestamps, counts:** Geist Mono Variable with tabular numerals.
- **Body:** Geist Variable with compact but readable line height.
- **Primary title scale:** Editor headings can be `text-2xl`; header brand text should sit around 15-16px.
- **Header control scale:** Header buttons and tabs should sit around 13px and 36-40px tall. They must not visually overpower the `wt studio` brand.
- **Dense metadata:** Use 12px or mono text only for paths, file names, IDs, and supporting context.
- **Letter spacing:** Use normal letter spacing for Korean and body UI. Use tracked uppercase only for tiny metadata badges, and avoid decorative all-caps.
- **Banned:** Inter, serif fonts, negative letter spacing, viewport-scaled font sizes, and oversized hero typography.

## 4. Component Styling
### Header
- The header is a compact command strip, not a toolbar hero.
- `wt studio`, `목록 열기`, `새로고침`, and surface tabs must share one optical scale.
- Header buttons use restrained neutral fills, 36-40px height, compact radius, and 13px labels.
- The product brand should not be smaller than surrounding command labels.
- Mobile header may wrap into two rows, but labels must never stack character-by-character.

### Navigation
- Use top segmented navigation for surfaces.
- Active surface is high-contrast neutral, not Signal Teal, so the save action remains visually distinct.
- Segments must use stable dimensions and `white-space: nowrap`.
- On mobile, surface navigation scrolls horizontally inside its rail. No horizontal page overflow.

### Resource Lists
- Resource lists are secondary navigation and should be hidden by default.
- Opening a list should not move the primary editor out of view on desktop.
- Empty list states should be short and factual. Do not display counts as the main message when the actionable state is empty.

### Editor Panels
- Use one main bordered surface per editing workflow.
- Avoid nested card stacks. Collapsed details may contain supporting metadata, but should feel subordinate.
- Editable content comes before final action.
- The save/status footer belongs after editable content, not next to the page title.

### Inputs
- Labels sit above inputs.
- Minimum touch target is 44px for form controls and final actions; compact header controls may be 36-40px.
- Focus ring uses Signal Teal only.
- Textareas need stable min-height; dynamic content must not resize the page unexpectedly.
- Do not show validation errors before the user has interacted with the relevant draft.

### Actions
- Primary action text must describe the user outcome: `저장`, not `적용`.
- There is one primary filled Signal Teal button per editing surface.
- Disabled primary actions can remain visible but should be visually quiet.
- Status guidance should sit near the save button so users can connect "why disabled" with "what to do next".

### Validation And Status
- Initial empty create state is not an error state.
- Blocking validation appears only after user interaction or after a failed save attempt.
- Readiness copy should say `저장할 수 있습니다`, not `적용할 수 있습니다`.
- Use `변경 미리보기`, not `Plan`.
- Use `변경 내용`, not `Diff` or `Unified diff`.
- Use `외부 이슈 연결`, `서비스`, and `이슈 번호/키`, not `출처 제공자`, `출처 ID`, or schema-shaped labels.

### Preview
- Preview appears only when there is a meaningful candidate change.
- Preview is secondary to editing and saving. It should not compete with the form.
- File paths may remain monospace metadata; internal API names should not appear in primary UI copy.

## 5. Layout Principles
- User flow is: choose surface, optionally open list, edit fields, read save status, save.
- Keep the editor as the dominant first-viewport object.
- Maximum content width is 96rem.
- Use CSS Grid for major layout and stable columns.
- Do not use card-inside-card decoration. If a group needs separation, use a divider, collapsed detail, or subtle background.
- Final save action belongs after the editable content.
- Status guidance belongs in the action area, not in the hero/header area.
- The drawer is a secondary panel on desktop and an optional stacked panel on mobile.
- Avoid full-width stretched editor lines when they hurt scanning; pair fields into columns only when labels remain clear.

## 6. Responsive Rules
- Mobile and narrow tablet layouts collapse form columns into one column.
- Header top controls stay on one row when possible; surface tabs move to a second row and scroll horizontally.
- No horizontal page overflow is allowed.
- Text inside buttons and tabs must never wrap into vertical Korean syllables.
- The primary save button may be full width on mobile.
- Status guidance stacks above the save button on mobile.
- Keep touch targets at least 44px outside compact header controls.

## 7. Motion & Interaction
- Motion is restrained and functional: transform and opacity only.
- Use active press feedback on all buttons.
- Hover and focus states should be visible but low-drama.
- Avoid scroll choreography, cinematic reveals, parallax, or decorative animation.
- Loading states should preserve layout dimensions; no generic centered spinner when a skeleton or stable placeholder would be clearer.

## 8. UX Writing Rules
- Prefer user-facing verbs: save, edit, choose, connect, review.
- Hide implementation terms unless they are exact file metadata.
- Error copy should be direct and actionable, not emotional.
- Do not show "success" enthusiasm with exclamation marks.
- Do not use internal model words in visible copy: `Plan`, `Diff`, `origin provider`, `baseline`, `fingerprint`, `precondition`.
- Korean labels should be short nouns or noun phrases. Avoid literal schema translations.
- Empty states should explain the next useful action, not the absence itself.

## 9. Accessibility Rules
- Every icon button needs an accessible label.
- Do not rely on color alone for validation; include text.
- Keyboard focus must be visible and use Signal Teal.
- Live regions are reserved for status guidance and validation updates.
- Disabled controls should remain readable.
- Interactive text must not be smaller than 13px in persistent navigation or 14px in body/form areas.

## 10. Anti-Patterns
- No AI purple or neon blue gradients.
- No pure black (#000000).
- No oversized header buttons beside tiny brand text.
- No character-stacked Korean labels.
- No icon-only ambiguous controls.
- No initial validation errors on untouched forms.
- No `Plan`, `Diff`, `Unified diff`, `origin provider`, or schema terms in primary UI copy.
- No centered hero, marketing layout, or oversized editorial intro.
- No nested card stacks for routine form sections.
- No decorative gradient orbs, bokeh, overlapping UI, or fake depth.
- No generic empty states like "No data" when a next action is available.
