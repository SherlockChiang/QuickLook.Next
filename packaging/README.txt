QuickLook Next Installer
========================

English
1. Double-click Install.cmd.
2. On the first install, approve the Windows UAC prompt. The script installs the
   included certificate into Local Computer > Trusted People, as required by Windows
   for sideloaded MSIX packages. Upgrades reuse existing trust and normally skip UAC.
3. It then installs or updates QuickLook Next for the current user from the signed MSIX.
4. QuickLook Next can be removed later from Windows Settings > Apps.

The certificate is a temporary project development certificate, not a commercial
identity certificate. Verify that this installer came from the official GitHub
release page: https://github.com/SherlockChiang/QuickLook.Next/releases

简体中文
1. 双击 Install-ZH-CN.cmd。
2. 首次安装时同意 Windows UAC 提权提示。脚本会按 Windows 侧载 MSIX 的要求，
   将随附证书安装到“本地计算机 > 受信任人”。升级会复用现有信任，通常不再提权。
3. 随后从已签名的 MSIX 为当前用户安装或更新 QuickLook Next。
4. 以后可在 Windows 设置 > 应用中卸载。

当前证书是项目临时开发证书，不是商业身份签名证书。请确认安装包来自官方
GitHub Release 页面：https://github.com/SherlockChiang/QuickLook.Next/releases
