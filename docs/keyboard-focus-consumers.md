# Keyboard Focus Ownership And Key Consumers

The preview uses two focus modes: the initial Explorer-open session transfers keyboard focus to
QuickLook, while later Explorer-originated switches stay non-activating so shell selection
follow-up keeps working. This note records which component consumes each key for each focus owner,
so new keys can be added against the same contract instead of reverse-engineering the two paths.

## Ownership model

1. The initial `ExplorerOpen` session created by Space activates the preview and may move focus to
   its content. The loading shell uses the normal activation path so focus does not depend on the
   preview type or on the final renderer.
2. `ExplorerSwitch` sessions reveal with `SW_SHOWNOACTIVATE` / `SWP_NOACTIVATE`, preserving shell
   focus for selection follow-up. `WS_EX_NOACTIVATE` is removed when the window is shown so a later
   click can still focus it.
3. Clicking the preview window transfers keyboard focus to the preview. Explorer-driven switching
   stops afterwards because the native hook only acts while Explorer is the foreground window.
4. `PreviewSession.ShouldActivateWindow` / `ShouldFocusContent` allow initial `ExplorerOpen` and
   `WindowNavigation` focus; `ExplorerSwitch` never calls `Activate()` or moves element focus.
5. The window controller may use a transient `HWND_TOPMOST` pulse to recover z-order on reveal or
   resize, but immediately demotes the window with `HWND_NOTOPMOST`; preview visibility must not
   leave the window permanently system-topmost.

## Consumer matrix

| Input | Explorer owns keyboard focus | Preview window owns keyboard focus |
| --- | --- | --- |
| Space | Hook toggles the preview: close when visible, open otherwise (`WM_QL_PREVIEW` / `WM_QL_CLOSE`) | `ClosePreviewFromKeyboard` unless the focused element uses Space |
| Arrow keys | Delayed selection switch: generation-checked 80 ms timer re-reads the Explorer selection (`WM_QL_SWITCH_DELAYED`) | In-preview content navigation (filmstrip siblings, list views) |
| Mouse click on an Explorer item | Delayed selection switch (`mouse_proc`) | Ownership transfer to the preview window |
| Esc | Close preview (`WM_QL_CLOSE`) | The hook closes the preview only when no text input in the preview has focus; the text search or filter box dismisses itself first without closing the preview |
| Ctrl+F | — (the hook ignores chords) | Text search (`OpenTextSearch`) or listing filter (`FocusFilter`) |
| F3 / Shift+F3 | — | Search next/previous |
| + / - (OEM or numpad) | Zoom in/out (`WM_QL_ZOOM_IN` / `WM_QL_ZOOM_OUT`) | — |
| F5 | Reload preview (bare key; works for either owner) | Same |
| F11 | Fullscreen toggle (bare key; works for either owner) | Same |

## Guardrails

- Hook-driven keys require no modifiers, suppress text inputs
  (`foreground_text_input_active` class check for Edit/RichEdit on the foreground window's
  focus), and — except F5/F11 and Esc — additionally require Explorer to be the foreground
  window. Esc is checked against the actual focus owner so dismissing the preview's own search
  or filter box never also closes the preview.
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
