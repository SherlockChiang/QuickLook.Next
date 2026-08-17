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

Never replace those sideload values with the Store identity. The Store launch
version is now deliberately `1.0.0.0`, generated from the repository's
semantic `VERSION=1.0.0`; the existing GitHub latest release remains `0.3.7`
with its signed sideload identity and update metadata. Future GitHub releases
can continue from the monotonic `1.0.0` semantic line.

## Identity and package checklist

Before building the first candidate, keep these exact values in the Store-only
configuration and review the remaining submission inputs:

- package identity name, publisher subject, and publisher display name (recorded
  above and supplied through repository variables);
- Store version (`1.0.0.0`, generated from `VERSION=1.0.0`);
- supported architecture(s) and minimum Windows build;
- Store listing languages and support URL;
- privacy-policy URL and contact address;
- age rating and capability declarations.

The Store build must regenerate `resources.pri` with the Store package identity;
changing only `AppxManifest.xml` is invalid because the PRI primary resource map
is identity-bound. The candidate must also pass the existing payload proof,
architecture guards, MSIX manifest validation, WACK, install, update, and
uninstall checks before upload.

## First real-identity candidate evidence

The first `1.0.0.0` candidate was built from commit
`14d11921040c1f7e5e04f15389f4bb023c3c1f78` on 2026-08-17. The local package
validation confirmed the Store manifest identity, an x64 architecture, an
unsigned package, and a `resources.pri` primary map named
`Uranus92.QuickLookNext`. The local artifact hashes were:

```text
MSIX:       a186bcca6ede4a2ba1c125429e161b3df94d7aa13ba5fd93694b478ac6994c9a
MSIXUPLOAD: b15ddb528ae2f38e90d49c802c85d6df82f37dae64f9ef615b95723cb022fa85
```

The manual `Microsoft Store Candidate` workflow run
[`32035898127`](https://github.com/SherlockChiang/QuickLook.Next/actions/runs/32035898127)
completed successfully from the same commit. It produced artifact ID
`9290943884` (`QuickLook.Next-Microsoft-Store-1`), whose outer artifact ZIP is
114,541,774 bytes with SHA-256
`c555fb10fc8cf17c88e2c0f7e26ab11988f0e1c0b8fae34b8612362e008eb660`.
GitHub attestation
[`41157969`](https://github.com/SherlockChiang/QuickLook.Next/attestations/41157969)
covers the MSIX, `.msixupload`, and metadata manifest as three subjects and is
anchored in the Sigstore Rekor transparency log. The ordinary CI run
[`32035883651`](https://github.com/SherlockChiang/QuickLook.Next/actions/runs/32035883651)
also passed its release checks, dependency audits, and website build.

No candidate was installed or uploaded to Partner Center. WACK and clean
install/update/uninstall testing therefore remain mandatory before submission.

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
