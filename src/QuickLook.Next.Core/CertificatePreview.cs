using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace QuickLook.Next.Core;

public static class CertificatePreview
{
    public const long MaxHandleInputBytes = 1024 * 1024;

    public static PreviewReady Create(string requestId, string path, long size)
    {
        string fileName = Path.GetFileName(path);
        if (IsCertificateBundle(fileName))
            return CreateFailure(requestId, fileName, size, "certificate bundles are not supported");
        try
        {
            using X509Certificate2 cert = X509CertificateLoader.LoadCertificateFromFile(path);
            return CreateReady(requestId, fileName, size, cert);
        }
        catch (CryptographicException)
        {
            return CreateFailure(requestId, fileName, size, "failed to parse certificate");
        }
        catch (IOException)
        {
            return CreateFailure(requestId, fileName, size, "could not read certificate");
        }
        catch (Exception)
        {
            return CreateFailure(requestId, fileName, size, "certificate preview failed");
        }
    }

    public static async Task<PreviewReady> CreateFromHandleAsync(
        string requestId,
        string logicalPath,
        SafeFileHandle sourceHandle,
        long sourceLength,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        string fileName = Path.GetFileName(logicalPath);
        if (IsCertificateBundle(fileName))
            return CreateFailure(requestId, fileName, sourceLength, "certificate bundles are not supported");
        if (sourceLength is < 0 or > MaxHandleInputBytes)
            return CreateFailure(requestId, fileName, sourceLength, "certificate exceeds the 1 MiB safety limit");
        if (sourceHandle.IsInvalid || sourceHandle.IsClosed)
            return CreateFailure(requestId, fileName, sourceLength, "certificate handle is unavailable");

        try
        {
            byte[] bytes = GC.AllocateUninitializedArray<byte>(checked((int)sourceLength));
            int offset = 0;
            while (offset < bytes.Length)
            {
                cancellationToken.ThrowIfCancellationRequested();
                int read = await RandomAccess.ReadAsync(
                    sourceHandle,
                    bytes.AsMemory(offset),
                    offset,
                    cancellationToken);
                if (read == 0)
                    return CreateFailure(requestId, fileName, sourceLength, "certificate input ended unexpectedly");
                offset = checked(offset + read);
            }
            cancellationToken.ThrowIfCancellationRequested();
            using X509Certificate2 cert = X509CertificateLoader.LoadCertificate(bytes);
            cancellationToken.ThrowIfCancellationRequested();
            return CreateReady(requestId, fileName, sourceLength, cert);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (CryptographicException)
        {
            return CreateFailure(requestId, fileName, sourceLength, "failed to parse certificate");
        }
        catch (IOException)
        {
            return CreateFailure(requestId, fileName, sourceLength, "could not read certificate");
        }
        catch (Exception)
        {
            return CreateFailure(requestId, fileName, sourceLength, "certificate preview failed");
        }
    }

    private static PreviewReady CreateReady(
        string requestId,
        string fileName,
        long size,
        X509Certificate2 cert)
    {
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

    private static PreviewReady CreateFailure(string requestId, string fileName, long size, string status)
        => new(requestId, "certificate", fileName, 640, 420)
        {
            TextContent = $"Name: {fileName}\nKind: certificate\nSize: {size:N0} bytes\nStatus: {status}",
            TextFormat = "plain",
            TextLanguage = "text",
        };

    private static bool IsCertificateBundle(string fileName)
    {
        string extension = Path.GetExtension(fileName);
        return extension.Equals(".p7b", StringComparison.OrdinalIgnoreCase)
            || extension.Equals(".p7c", StringComparison.OrdinalIgnoreCase);
    }
}
