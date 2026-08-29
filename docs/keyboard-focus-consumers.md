# Keyboard Focus Ownership And Key Consumers

The preview is a non-activating overlay: Explorer-originated previews must leave keyboard
focus in the shell so arrow-key selection follow-up keeps working. This note records which
component consumes each key for each focus owner, so new keys can be added against the same
contract instead of reverse-engineering the two paths.

## Ownership model

1. Explorer-originated sessions (`PreviewNavigationSource.ExplorerOpen` / `ExplorerSwitch`)
   reveal the window with `SW_SHOWNOACTIVATE` / `SWP_NOACTIVATE`. `WS_EX_NOACTIVATE` is removed
   when the window is shown so a later click can still focus it.
2. Clicking the preview window transfers keyboard focus to the preview. This is the intended
   ownership hand-off; Explorer-driven switching stops afterwards because the native hook only
   acts while Explorer is the foreground window.
3. `PreviewSession.ShouldActivateWindow` / `ShouldFocusContent` gate window activation and
   element-level focus to `WindowNavigation` sessions only. Explorer-originated sessions never
   call `Activate()` and never move element focus.

## Consumer matrix

| Input | Explorer owns keyboard focus | Preview window owns keyboard focus |
| --- | --- | --- |
| Space | Hook toggles the preview: close when visible, open otherwise (`WM_QL_PREVIEW` / `WM_QL_CLOSE`) | `ClosePreviewFromKeyboard` unless the focused element uses Space |
| Arrow keys | Delayed selection switch: generation-checked 80 ms timer re-reads the Explorer selection (`WM_QL_SWITCH_DELAYED`) | In-preview content navigation (filmstrip siblings, list views) |
| Mouse click on an Explorer item | Delayed selection switch (`mouse_proc`) | Ownership transfer to the preview window |
| Esc | Close preview (`WM_QL_CLOSE`) | The hook still sees Esc while the preview is visible and closes the preview; the text search box additionally closes search first |
| Ctrl+F | — (the hook ignores chords) | Text search (`OpenTextSearch`) or listing filter (`FocusFilter`) |
| F3 / Shift+F3 | — | Search next/previous |
| + / - (OEM or numpad) | Zoom in/out (`WM_QL_ZOOM_IN` / `WM_QL_ZOOM_OUT`) | — |
| F5 | Reload preview (bare key; works for either owner) | Same |
| F11 | Fullscreen toggle (bare key; works for either owner) | Same |

## Guardrails

- Hook-driven keys require no modifiers, suppress Explorer text inputs
  (`explorer_text_input_active` class check for Edit/RichEdit), and — except F5/F11 and Esc —
  additionally require Explorer to be the foreground window.
- Switch messages carry `PREVIEW_VISIBILITY_GENERATION`. The pump re-validates with
  `accepts_switch_event` before arming the 80 ms timer, and `switch_timer_proc` re-checks
  `SWITCH_GENERATION` and `PREVIEW_VISIBLE` at fire time. Hiding posts `WM_QL_CANCEL_SWITCH`;
  the generation re-check is the second layer that also covers a lost cancel message (for
  example a hide before the hook thread reports ready). `OPEN`/`CLOSE` intents invalidate any
  armed timer.
- Duplicate close intents (hook and window key handling can both fire for one physical key)
  are deduplicated by `_keyboardCloseQueued` and the duplicate open/close reveal guard in
  `MainWindow`.

## Code pointers

- Hook thread, key classification, and switch timer: `native/quicklook_next_native/src/lib.rs`
  (`keyboard_proc`, `mouse_proc`, `hook_thread`, `switch_timer_proc`, `accepts_switch_event`).
- Preview-window key handling: `src/QuickLook.Next.App/MainWindow.xaml.cs` (`OnRootGridKeyDown`).
- Focus policy: `src/QuickLook.Next.App/PreviewSession.cs`.
- Window activation styles: `src/QuickLook.Next.App/PreviewWindowController.cs`.
