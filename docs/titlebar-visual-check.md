# Title-Bar Inset Visual Check

This checklist is the manual evidence gate for custom-title-bar inset changes.
The structural guard verifies event wiring and padding policy, but it cannot
prove that Windows caption buttons, localized text, scaling, and accessibility
colors render correctly on a real desktop.

Do not state that the title-bar visual check passed unless every required row
below has both a screenshot and a completed run record attached to the change,
release record, or another immutable review artifact. A build or structural
guard passing by itself is not a manual visual pass.

## Test setup and evidence

Use the packaged build being reviewed. Record all of these fields once per run:

| Field | Required value |
|---|---|
| Date | Local date in `YYYY-MM-DD` form |
| Tester | Name or review identity |
| Commit | Full Git commit SHA |
| App version | Installed four-part package version |
| Windows edition | For example, Windows 11 Pro |
| OS build | Full `major.minor.build.revision` value |
| Monitor layout | Resolution, ordering, and primary-monitor identity |
| Scale | Effective percentage and `XamlRoot.RasterizationScale` for each screenshot |
| Window size | Logical width × height |
| Language | Exact application language |
| Contrast/theme | High Contrast scheme or normal light/dark theme |
| Result | Pass or fail, with issue/link when failed |
| Screenshot | Exact attached filename or immutable artifact link |

Save each screenshot as:

```text
titlebar-<window>-<scenario>-win<build>-scale<percent>-<YYYYMMDD>.png
```

Use `main`, `settings`, or `welcome` for `<window>`, and `compact`,
`cross-monitor-200`, `zh-cn`, or `high-contrast` for `<scenario>`. Replace dots
in the Windows build with hyphens. Example:

```text
titlebar-settings-cross-monitor-200-win10-0-26100-4652-scale200-20260731.png
```

The cross-monitor check requires a pair of screenshots. Append `-before100`
and `-after200` before the `.png` suffix so both states remain traceable.
For example:

```text
titlebar-settings-cross-monitor-200-win10-0-26100-4652-scale100-20260731-before100.png
titlebar-settings-cross-monitor-200-win10-0-26100-4652-scale200-20260731-after200.png
```

Capture the whole window, including every caption button and both outer edges.
Do not crop away an overlap, clipped title, or unexpected blank strip.

## Required matrix

Complete all 12 checks. Use the same build and commit throughout a run.

| Window | Compact width | Move across a 200% display boundary | Simplified Chinese (`zh-CN`) | Windows High Contrast |
|---|---:|---:|---:|---:|
| Main preview window | Required | Required | Required | Required |
| Settings window | Required | Required | Required | Required |
| Welcome window | Required | Required | Required | Required |

For the compact-width rows, resize to the narrowest supported width without
forcing the operating system to violate the window minimum. Record the actual
logical width. For cross-monitor rows, start fully on a 100% display, capture
the recorded scale and `before100` screenshot, move the window fully onto a
200% display, wait for layout to settle, then capture the `after200` screenshot
and new scale. Do not resize the window between those two observations.

For the `zh-CN` rows, select Simplified Chinese in the app and reopen the target
window before capturing it. For High Contrast rows, enable a Windows High
Contrast scheme before opening the target window and record the scheme name.

## Pass criteria

Every screenshot must show all of the following:

- The title, app icon where present, and interactive title-bar content stay outside the
  operating system caption-button regions on both the left and right.
- Left and right content spacing is visually based on the window's normal
  symmetric padding; there is no residual 140/144 DIP fixed blank strip.
- Enabled caption buttons remain visible, correctly aligned, and perform their
  expected actions. The intentionally disabled maximize buttons in Settings
  and Welcome remain visibly disabled and do not respond.
- Moving onto the 200% display updates spacing once layout settles; spacing is
  not doubled, halved, stale from the first monitor, or clipped.
- Simplified Chinese title text is complete and does not overlap caption
  buttons, icons, or adjacent controls.
- High Contrast preserves readable foreground/background separation for the
  title and caption buttons, with no invisible hover/pressed region.
- No title-bar element jumps, clips, or remains offset after opening, resizing,
  changing scale, changing language, or enabling High Contrast.

Any failed row blocks the manual visual-pass claim until it is fixed and the
affected row is recaptured. If a scenario cannot be executed because the
required monitor, OS mode, or language is unavailable, record it as **not run**,
not passed.

The result notes must also record that caption-button hover, pressed, click,
window drag, and disabled-button behavior were exercised. A screenshot alone
does not prove those interactions.

## Run record

Copy this table into the review or release evidence and complete one row per
screenshot:

| Date | Commit | Window | Scenario | OS build | Scale % / XamlRoot scale | Logical size | Language | Contrast/theme | Screenshot | Result/notes |
|---|---|---|---|---|---|---|---|---|---|---|
| | | | | | | | | | | |

The reviewer should verify the 15 screenshots and records (including three
cross-monitor before/after pairs), commit SHA, app version, OS build, date, and
scale values before accepting the manual visual check.
