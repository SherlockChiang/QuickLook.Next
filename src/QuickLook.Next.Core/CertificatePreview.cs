using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using System.Text;

namespace QuickLook.Next.Core;

public static class CertificatePreview
{
    public static PreviewReady Create(string requestId, string path, long size)
    {
        string fileName = Path.GetFileName(path);
        try
        {
            using X509Certificate2 cert = X509CertificateLoader.LoadCertificateFromFile(path);
            string[] usages = cert.Extensions
                .OfType<X509EnhancedKeyUsageExtension>()
                .SelectMany(extension => extension.EnhancedKeyUsages.Cast<Oid>())
                .Select(oid => string.IsNullOrWhiteSpace(oid.FriendlyName)
                    ? oid.Value ?? ""
                    : $"{oid.FriendlyName} ({oid.Value})")
                .Where(value => !string.IsNullOrWhiteSpace(value))
                .ToArray();

            var builder = new StringBuilder();
            builder.AppendLine($"Name: {fileName}");
            builder.AppendLine("Kind: certificate");
            builder.AppendLine($"Subject: {cert.Subject}");
            builder.AppendLine($"Issuer: {cert.Issuer}");
            builder.AppendLine($"Serial number: {cert.SerialNumber}");
            builder.AppendLine($"Thumbprint: {cert.Thumbprint}");
            builder.AppendLine($"Valid from: {cert.NotBefore:G}");
            builder.AppendLine($"Valid until: {cert.NotAfter:G}");
            builder.AppendLine($"Signature algorithm: {cert.SignatureAlgorithm.FriendlyName ?? cert.SignatureAlgorithm.Value}");
            builder.AppendLine($"Public key: {cert.PublicKey.Oid.FriendlyName ?? cert.PublicKey.Oid.Value}");
            builder.AppendLine($"Has private key: {(cert.HasPrivateKey ? "yes" : "no")}");
            if (usages.Length > 0)
                builder.AppendLine($"Enhanced key usage: {string.Join(", ", usages)}");
            builder.AppendLine($"File size: {size:N0} bytes");

            return new PreviewReady(requestId, "certificate", $"{fileName} - {cert.GetNameInfo(X509NameType.SimpleName, false)}", 720, 520)
            {
                TextContent = builder.ToString(),
                TextFormat = "plain",
                TextLanguage = "text",
            };
        }
        catch (Exception ex)
        {
            return new PreviewReady(requestId, "certificate", fileName, 640, 420)
            {
                TextContent = $"Name: {fileName}\nKind: certificate\nSize: {size:N0} bytes\nStatus: failed to parse certificate\nError: {ex.Message}",
                TextFormat = "plain",
                TextLanguage = "text",
            };
        }
    }
}
