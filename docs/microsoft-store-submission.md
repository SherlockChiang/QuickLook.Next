# Microsoft Store submission plan

This repository has two intentionally separate Windows distribution channels:

- GitHub Releases keep the current signed sideload package and its pinned
  `CN=QuickLook Next Development` certificate.
- Microsoft Store receives a Store-identity package that Microsoft re-signs for
  customers after certification.

The split is required by the Store package rules and prevents a Store identity
change from breaking existing sideload upgrades.

## Confirmed Microsoft requirements

The current Microsoft Learn guidance says:

- Store MSIX submissions do not need a CA-trusted certificate. Microsoft
  re-signs the package after certification.
- A Store package's fourth MSIX version component is reserved for Store use and
  must be `0` when uploaded. The first component must not be `0`.
- The manifest values for package identity are case-sensitive and must exactly
  match the values shown by Partner Center's **View app identity details**.
- `.msix`, `.msixbundle`, and `.msixupload` are supported package forms;
  `.msixupload` is the preferred submission container when symbols are supplied.
- Windows App Certification Kit (WACK) should be run before submission.

References:

- [App package requirements for MSIX apps](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements)
- [Code signing options for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)
- [Microsoft Store publish overview](https://learn.microsoft.com/en-us/windows/apps/publish/)

## Current Store product

The `QuickLook Next` product name has been reserved in Partner Center as an
`MSIX or PWA app` (product ID `9PM0XKBFJC6R`). The Store identity is now
available and must be passed to the Store-only packaging workflow:

```text
Package/Identity/Name:                 Uranus92.QuickLookNext
Package/Identity/Publisher:            CN=FDE9F71C-A397-410B-81EE-36E18F4325E5
Package/Properties/PublisherDisplayName: Uranus92
Store ID:                              9PM0XKBFJC6R
Store page:                            https://apps.microsoft.com/detail/9PM0XKBFJC6R
```

Configure the three repository variables consumed by
`.github/workflows/store-package.yml` with the exact values above:

```text
MICROSOFT_STORE_PACKAGE_IDENTITY_NAME=Uranus92.QuickLookNext
MICROSOFT_STORE_PUBLISHER=CN=FDE9F71C-A397-410B-81EE-36E18F4325E5
MICROSOFT_STORE_PUBLISHER_DISPLAY_NAME=Uranus92
```

The current package manifest intentionally remains the sideload identity:

```text
Name:              SherlockChiang.QuickLookNext
Publisher:         CN=QuickLook Next Development
PublisherDisplayName: QuickLook Next
```

Never replace those sideload values with the Store identity. The current
product version is `0.3.7`; it cannot be used directly as a Store version
because its first component is `0`. The Store launch should therefore be
planned as a deliberate `1.0.0.0` package (or another explicitly documented
monotonic Store version), while GitHub remains on the semantic release line.

## Identity and package checklist

Before building the first candidate, keep these exact values in the Store-only
configuration and review the remaining submission inputs:

- package identity name, publisher subject, and publisher display name (recorded
  above and supplied through repository variables);
- Store version (`X.Y.Z.0`);
- supported architecture(s) and minimum Windows build;
- Store listing languages and support URL;
- privacy-policy URL and contact address;
- age rating and capability declarations.

The Store build must regenerate `resources.pri` with the Store package identity;
changing only `AppxManifest.xml` is invalid because the PRI primary resource map
is identity-bound. The candidate must also pass the existing payload proof,
architecture guards, MSIX manifest validation, WACK, install, update, and
uninstall checks before upload.

## Listing material still needed

- English, Simplified Chinese, and Traditional Chinese descriptions;
- short description and feature highlights;
- screenshots at the Store-requested dimensions;
- Store hero/icon assets derived from the existing icon set;
- support URL and privacy-policy URL;
- explanation for `runFullTrust` and startup-task behavior;
- a clean-install and upgrade test report.

No Partner Center submission or product reservation is performed by the local
build until the identity values and account access are confirmed.
