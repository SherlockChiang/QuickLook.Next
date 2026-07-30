using System.Globalization;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

/// <summary>
/// Optional Windows Property Handler metadata bound to an independently reopened source HANDLE.
/// The logical basename selects a machine-wide handler; it is never resolved or opened as a path.
/// </summary>
internal static class WindowsPropertyHandlerMetadataReader
{
    private const long MaxInputImageBytes = 256L * 1024 * 1024;
    private const int MaxProperties = 128;
    private const int MaxAcceptedProperties = 48;
    private const int MaxCanonicalNameChars = 128;
    private const int MaxStringChars = 512;
    private const int MaxAggregateStringChars = 4 * 1024;
    private const int PropertyHandlerTimeoutExitCode = 32;
    private static readonly TimeSpan DrainGrace = TimeSpan.FromMilliseconds(250);
    private static readonly SemaphoreSlim MetadataGate = new(1, 1);
    private static readonly object HandlerModuleGate = new();
    private static nint _handlerModule;

    private static readonly HashSet<string> AllowedExtensions = new(StringComparer.Ordinal)
    {
        ".avif", ".bmp", ".dib", ".gif", ".heic", ".heif", ".ico", ".jfif", ".jpe",
        ".jpeg", ".jpg", ".jxl", ".png", ".tif", ".tiff", ".webp",
    };

    private static readonly HashSet<string> SupportedPropertyNames = new(StringComparer.Ordinal)
    {
        "System.ApplicationName",
        "System.Comment",
        "System.GPS.Altitude",
        "System.GPS.ImgDirection",
        "System.GPS.LatitudeDecimal",
        "System.GPS.LongitudeDecimal",
        "System.Image.BitDepth",
        "System.Image.ColorSpace",
        "System.Image.CompressionText",
        "System.Image.HorizontalResolution",
        "System.Image.HorizontalSize",
        "System.Image.VerticalResolution",
        "System.Image.VerticalSize",
        "System.Media.Duration",
        "System.Media.FrameCount",
        "System.Photo.CameraManufacturer",
        "System.Photo.CameraModel",
        "System.Photo.CameraSerialNumber",
        "System.Photo.Contrast",
        "System.Photo.DateTaken",
        "System.Photo.DigitalZoom",
        "System.Photo.EXIFVersion",
        "System.Photo.ExposureBias",
        "System.Photo.ExposureProgram",
        "System.Photo.ExposureTime",
        "System.Photo.FNumber",
        "System.Photo.Flash",
        "System.Photo.FocalLength",
        "System.Photo.FocalLengthInFilm",
        "System.Photo.GainControl",
        "System.Photo.ISOSpeed",
        "System.Photo.LensManufacturer",
        "System.Photo.LensModel",
        "System.Photo.LightSource",
        "System.Photo.MaxAperture",
        "System.Photo.MeteringMode",
        "System.Photo.Orientation",
        "System.Photo.PhotometricInterpretationText",
        "System.Photo.Saturation",
        "System.Photo.Sharpness",
        "System.Photo.SubjectDistance",
        "System.Photo.WhiteBalance",
        "System.SoftwareUsed",
        "System.Title",
    };

    public static async Task<ImageMetadata?> TryReadHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        if (sourceLength is < 0 or > MaxInputImageBytes
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed
            || timeout <= TimeSpan.Zero)
        {
            return null;
        }

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);
        bool enteredGate = false;
        Task<ImageMetadata?>? worker = null;
        try
        {
            await MetadataGate.WaitAsync(timeoutCts.Token);
            enteredGate = true;
            worker = Task.Run(
                () => ReadHandle(
                    sourceHandle,
                    sourceLength,
                    logicalName,
                    timeoutCts.Token),
                CancellationToken.None);
            return await worker.WaitAsync(timeoutCts.Token);
        }
        catch (OperationCanceledException) when (
            !cancellationToken.IsCancellationRequested
            && timeoutCts.IsCancellationRequested)
        {
            if (worker is not null)
                await DrainWorkerOrExitAsync(worker);
            DiagLog.Write(
                "RasterHost",
                $"Property Handler metadata timed out; timeout={timeout.TotalMilliseconds:0}ms");
            return null;
        }
        catch (OperationCanceledException)
        {
            if (worker is not null)
                await DrainWorkerOrExitAsync(worker);
            throw;
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", $"Property Handler metadata failed: {ex.Message}");
            return null;
        }
        finally
        {
            if (enteredGate)
                MetadataGate.Release();
        }
    }

    public static ImageMetadata? Merge(ImageMetadata? primary, ImageMetadata? supplement)
    {
        if (primary is null)
            return supplement;
        if (supplement is null)
            return primary;

        return primary with
        {
            Format = FirstText(primary.Format, supplement.Format),
            Title = FirstText(primary.Title, supplement.Title),
            Comment = FirstText(primary.Comment, supplement.Comment),
            Make = FirstText(primary.Make, supplement.Make),
            Model = FirstText(primary.Model, supplement.Model),
            DateTime = FirstText(primary.DateTime, supplement.DateTime),
            Width = primary.Width ?? supplement.Width,
            Height = primary.Height ?? supplement.Height,
            Orientation = primary.Orientation ?? supplement.Orientation,
            LensMake = FirstText(primary.LensMake, supplement.LensMake),
            LensModel = FirstText(primary.LensModel, supplement.LensModel),
            Software = FirstText(primary.Software, supplement.Software),
            FNumber = primary.FNumber ?? supplement.FNumber,
            MaxAperture = primary.MaxAperture ?? supplement.MaxAperture,
            ExposureTime = primary.ExposureTime ?? supplement.ExposureTime,
            Iso = primary.Iso ?? supplement.Iso,
            FocalLength = primary.FocalLength ?? supplement.FocalLength,
            FocalLengthIn35mmFilm =
                primary.FocalLengthIn35mmFilm ?? supplement.FocalLengthIn35mmFilm,
            ExposureBias = primary.ExposureBias ?? supplement.ExposureBias,
            ExposureProgram = primary.ExposureProgram ?? supplement.ExposureProgram,
            ExposureMode = primary.ExposureMode ?? supplement.ExposureMode,
            MeteringMode = primary.MeteringMode ?? supplement.MeteringMode,
            Flash = primary.Flash ?? supplement.Flash,
            WhiteBalance = primary.WhiteBalance ?? supplement.WhiteBalance,
            LightSource = primary.LightSource ?? supplement.LightSource,
            DigitalZoomRatio = primary.DigitalZoomRatio ?? supplement.DigitalZoomRatio,
            SubjectDistance = primary.SubjectDistance ?? supplement.SubjectDistance,
            Contrast = primary.Contrast ?? supplement.Contrast,
            Saturation = primary.Saturation ?? supplement.Saturation,
            Sharpness = primary.Sharpness ?? supplement.Sharpness,
            GainControl = primary.GainControl ?? supplement.GainControl,
            ColorSpace = primary.ColorSpace ?? supplement.ColorSpace,
            ExifVersion = FirstText(primary.ExifVersion, supplement.ExifVersion),
            CameraSerial = FirstText(primary.CameraSerial, supplement.CameraSerial),
            LensSerial = FirstText(primary.LensSerial, supplement.LensSerial),
            Latitude = primary.Latitude ?? supplement.Latitude,
            Longitude = primary.Longitude ?? supplement.Longitude,
            Altitude = primary.Altitude ?? supplement.Altitude,
            Direction = primary.Direction ?? supplement.Direction,
            HorizontalResolution =
                primary.HorizontalResolution ?? supplement.HorizontalResolution,
            VerticalResolution = primary.VerticalResolution ?? supplement.VerticalResolution,
            PhotometricInterpretation = FirstText(
                primary.PhotometricInterpretation,
                supplement.PhotometricInterpretation),
            BitDepth = primary.BitDepth ?? supplement.BitDepth,
            ColorType = FirstText(primary.ColorType, supplement.ColorType),
            Compression = FirstText(primary.Compression, supplement.Compression),
            HasAlpha = primary.HasAlpha ?? supplement.HasAlpha,
            Interlace = FirstText(primary.Interlace, supplement.Interlace),
            Animated = primary.Animated ?? supplement.Animated,
            FrameCount = primary.FrameCount ?? supplement.FrameCount,
            DurationMs = primary.DurationMs ?? supplement.DurationMs,
        };
    }

    private static async Task DrainWorkerOrExitAsync(Task worker)
    {
        try
        {
            await worker.WaitAsync(DrainGrace);
        }
        catch (TimeoutException)
        {
            DiagLog.Write(
                "RasterHost",
                "Property Handler metadata did not drain after cancellation; exiting host.");
            Environment.Exit(PropertyHandlerTimeoutExitCode);
        }
        catch
        {
            // The original cancellation or provider failure is reported by the caller.
        }
    }

    private static ImageMetadata? ReadHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (Marshal.SizeOf<PropertyKey>() != 20
            || Marshal.SizeOf<PropVariant>() != 24
            || Marshal.OffsetOf<PropVariant>(nameof(PropVariant.Pointer)).ToInt32() != 8)
        {
            return null;
        }
        HandlerRegistration? registration = PropertyHandlerResolver.TryResolve(logicalName);
        if (registration is null)
            return null;

        int initializeResult = PropertyNative.CoInitializeEx(0, PropertyNative.CoinitMultithreaded);
        if (initializeResult < 0)
            return null;

        try
        {
            using SafeFileHandle metadataHandle =
                WindowsHandleTransfer.ReopenReadOnlyFile(sourceHandle, sourceLength);
            using var fileStream =
                new FileStream(metadataHandle, FileAccess.Read, 64 * 1024, isAsync: false);
            if (fileStream.Length != sourceLength)
                return null;

            using var stream = new ReadOnlyComStream(
                fileStream,
                sourceLength,
                cancellationToken);
            object? handler = null;
            nint handlerPointer = 0;
            try
            {
                if (!TryCreateHandler(
                    registration.Value,
                    out handler,
                    out handlerPointer))
                {
                    return null;
                }
                if (handler is not IInitializeWithStream initializer
                    || handler is not IPropertyStore propertyStore)
                {
                    return null;
                }

                int result = initializer.Initialize(stream, PropertyNative.StgmRead);
                if (result < 0)
                    return null;

                return ReadProperties(propertyStore, cancellationToken);
            }
            finally
            {
                if (handlerPointer != 0)
                    Marshal.Release(handlerPointer);
                if (handler is not null && Marshal.IsComObject(handler))
                {
                    try { Marshal.FinalReleaseComObject(handler); }
                    catch { }
                }
                GC.KeepAlive(stream);
            }
        }
        finally
        {
            PropertyNative.CoUninitialize();
        }
    }

    private static bool TryCreateHandler(
        HandlerRegistration registration,
        out object? handler,
        out nint handlerPointer)
    {
        handler = null;
        handlerPointer = 0;
        nint handlerModule = GetOrLoadHandlerModule(registration.ModulePath);
        if (handlerModule == 0)
            return false;

        object? factory = null;
        nint factoryPointer = 0;
        bool activated = false;
        try
        {
            nint entryPoint = PropertyNative.GetProcAddress(
                handlerModule,
                "DllGetClassObject");
            if (entryPoint == 0)
                return false;

            var getClassObject =
                Marshal.GetDelegateForFunctionPointer<DllGetClassObjectDelegate>(entryPoint);
            Guid clsid = registration.Clsid;
            Guid classFactoryIid = PropertyNative.IidClassFactory;
            int result = getClassObject(
                ref clsid,
                ref classFactoryIid,
                out factoryPointer);
            if (result < 0 || factoryPointer == 0)
                return false;

            factory = Marshal.GetObjectForIUnknown(factoryPointer);
            Marshal.Release(factoryPointer);
            factoryPointer = 0;
            if (factory is not IClassFactory classFactory)
                return false;

            Guid handlerIid = PropertyNative.IidIUnknown;
            result = classFactory.CreateInstance(0, ref handlerIid, out handlerPointer);
            if (result < 0 || handlerPointer == 0)
                return false;

            handler = Marshal.GetObjectForIUnknown(handlerPointer);
            Marshal.Release(handlerPointer);
            handlerPointer = 0;
            activated = true;
            return true;
        }
        finally
        {
            if (factoryPointer != 0)
                Marshal.Release(factoryPointer);
            if (factory is not null && Marshal.IsComObject(factory))
            {
                try { Marshal.FinalReleaseComObject(factory); }
                catch { }
            }
            if (!activated)
            {
                if (handlerPointer != 0)
                {
                    Marshal.Release(handlerPointer);
                    handlerPointer = 0;
                }
                if (handler is not null && Marshal.IsComObject(handler))
                {
                    try { Marshal.FinalReleaseComObject(handler); }
                    catch { }
                    handler = null;
                }
            }
        }
    }

    private static nint GetOrLoadHandlerModule(string modulePath)
    {
        lock (HandlerModuleGate)
        {
            if (_handlerModule != 0)
                return _handlerModule;
            _handlerModule = PropertyNative.LoadLibraryEx(
                modulePath,
                0,
                PropertyNative.LoadLibrarySearchSystem32);
            return _handlerModule;
        }
    }

    private static ImageMetadata? ReadProperties(
        IPropertyStore propertyStore,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        int result = propertyStore.GetCount(out uint propertyCount);
        if (result < 0)
            return null;

        var metadata = new MetadataAccumulator();
        uint count = Math.Min(propertyCount, MaxProperties);
        for (uint index = 0; index < count && metadata.Accepted < MaxAcceptedProperties; index++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            result = propertyStore.GetAt(index, out PropertyKey key);
            if (result < 0)
                continue;

            string? canonicalName = GetCanonicalName(ref key);
            if (canonicalName is null || !SupportedPropertyNames.Contains(canonicalName))
                continue;

            PropVariant value = default;
            try
            {
                result = propertyStore.GetValue(ref key, out value);
                if (result >= 0)
                    metadata.Apply(canonicalName, ref value);
            }
            finally
            {
                PropertyNative.PropVariantClear(ref value);
            }
        }

        return metadata.Build();
    }

    private static string? GetCanonicalName(ref PropertyKey key)
    {
        nint namePointer = 0;
        try
        {
            int result = PropertyNative.PSGetNameFromPropertyKey(ref key, out namePointer);
            if (result < 0 || namePointer == 0)
                return null;
            return ReadNullTerminatedUnicode(namePointer, MaxCanonicalNameChars);
        }
        finally
        {
            if (namePointer != 0)
                Marshal.FreeCoTaskMem(namePointer);
        }
    }

    private static unsafe string? ReadNullTerminatedUnicode(nint pointer, int maxChars)
    {
        if (pointer == 0)
            return null;
        char* value = (char*)pointer;
        int length = 0;
        while (length <= maxChars && value[length] != '\0')
            length++;
        return length is > 0 && length <= maxChars
            ? new string(value, 0, length)
            : null;
    }

    private static unsafe string? ReadNullTerminatedAnsi(nint pointer, int maxBytes)
    {
        if (pointer == 0)
            return null;
        byte* value = (byte*)pointer;
        int length = 0;
        while (length <= maxBytes && value[length] != 0)
            length++;
        return length is > 0 && length <= maxBytes
            ? Marshal.PtrToStringAnsi(pointer, length)
            : null;
    }

    private static string? ReadString(ref PropVariant value)
    {
        string? text = value.VariantType switch
        {
            PropertyNative.VtLpwstr =>
                ReadNullTerminatedUnicode(value.Pointer, MaxStringChars),
            PropertyNative.VtLpstr =>
                ReadNullTerminatedAnsi(value.Pointer, MaxStringChars),
            PropertyNative.VtBstr => ReadBstr(value.Pointer),
            _ => null,
        };
        return SanitizeText(text);
    }

    private static string? ReadBstr(nint pointer)
    {
        if (pointer == 0)
            return null;
        uint length = PropertyNative.SysStringLen(pointer);
        return length is > 0 and <= MaxStringChars
            ? Marshal.PtrToStringUni(pointer, checked((int)length))
            : null;
    }

    private static string? SanitizeText(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
            return null;

        string trimmed = value.Trim();
        var output = new StringBuilder(Math.Min(trimmed.Length, MaxStringChars));
        bool previousSpace = false;
        foreach (char character in trimmed)
        {
            if (output.Length >= MaxStringChars)
                break;
            if (char.IsControl(character) || char.IsWhiteSpace(character))
            {
                if (!previousSpace)
                    output.Append(' ');
                previousSpace = true;
                continue;
            }
            output.Append(character);
            previousSpace = false;
        }

        string result = output.ToString().Trim();
        if (result.Length > 0
            && char.IsHighSurrogate(result[^1]))
        {
            result = result[..^1];
        }
        return result.Length > 0 ? result : null;
    }

    private static bool TryReadUnsigned(ref PropVariant value, out ulong number)
    {
        switch (value.VariantType)
        {
            case PropertyNative.VtUi1:
                number = value.Ui1;
                return true;
            case PropertyNative.VtUi2:
                number = value.Ui2;
                return true;
            case PropertyNative.VtUi4:
                number = value.Ui4;
                return true;
            case PropertyNative.VtUi8:
                number = value.Ui8;
                return true;
            case PropertyNative.VtI1 when value.I1 >= 0:
                number = (ulong)value.I1;
                return true;
            case PropertyNative.VtI2 when value.I2 >= 0:
                number = (ulong)value.I2;
                return true;
            case PropertyNative.VtI4 when value.I4 >= 0:
                number = (ulong)value.I4;
                return true;
            case PropertyNative.VtI8 when value.I8 >= 0:
                number = (ulong)value.I8;
                return true;
            default:
                number = 0;
                return false;
        }
    }

    private static bool TryReadDouble(ref PropVariant value, out double number)
    {
        switch (value.VariantType)
        {
            case PropertyNative.VtR4:
                number = value.R4;
                break;
            case PropertyNative.VtR8:
                number = value.R8;
                break;
            default:
                if (!TryReadUnsigned(ref value, out ulong integer))
                {
                    number = 0;
                    return false;
                }
                number = integer;
                break;
        }
        return double.IsFinite(number);
    }

    private static string? ReadDateTime(ref PropVariant value)
    {
        try
        {
            DateTime dateTime = value.VariantType switch
            {
                PropertyNative.VtFileTime => DateTime.FromFileTimeUtc(value.FileTime),
                PropertyNative.VtDate when double.IsFinite(value.R8) =>
                    DateTime.FromOADate(value.R8),
                _ => default,
            };
            return dateTime == default
                ? null
                : dateTime.ToString("O", CultureInfo.InvariantCulture);
        }
        catch (ArgumentException)
        {
            return null;
        }
    }

    private static string? FirstText(string? primary, string? fallback)
        => !string.IsNullOrWhiteSpace(primary) ? primary : fallback;

    private sealed class MetadataAccumulator
    {
        private int _aggregateStringChars;
        private string? _title;
        private string? _comment;
        private string? _make;
        private string? _model;
        private string? _dateTime;
        private uint? _width;
        private uint? _height;
        private ushort? _orientation;
        private string? _lensMake;
        private string? _lensModel;
        private string? _software;
        private double? _fNumber;
        private double? _maxAperture;
        private double? _exposureTime;
        private uint? _iso;
        private double? _focalLength;
        private uint? _focalLengthIn35mmFilm;
        private double? _exposureBias;
        private ushort? _exposureProgram;
        private ushort? _meteringMode;
        private ushort? _flash;
        private ushort? _whiteBalance;
        private ushort? _lightSource;
        private double? _digitalZoomRatio;
        private double? _subjectDistance;
        private ushort? _contrast;
        private ushort? _saturation;
        private ushort? _sharpness;
        private ushort? _gainControl;
        private ushort? _colorSpace;
        private string? _exifVersion;
        private string? _cameraSerial;
        private double? _latitude;
        private double? _longitude;
        private double? _altitude;
        private double? _direction;
        private double? _horizontalResolution;
        private double? _verticalResolution;
        private string? _photometricInterpretation;
        private byte? _bitDepth;
        private string? _compression;
        private bool? _animated;
        private uint? _frameCount;
        private uint? _durationMs;

        public int Accepted { get; private set; }

        public void Apply(string canonicalName, ref PropVariant value)
        {
            bool accepted = canonicalName switch
            {
                "System.Title" => SetText(ref _title, ReadString(ref value)),
                "System.Comment" => SetText(ref _comment, ReadString(ref value)),
                "System.Photo.CameraManufacturer" => SetText(ref _make, ReadString(ref value)),
                "System.Photo.CameraModel" => SetText(ref _model, ReadString(ref value)),
                "System.Photo.DateTaken" => SetText(ref _dateTime, ReadDateTime(ref value)),
                "System.Photo.LensManufacturer" => SetText(ref _lensMake, ReadString(ref value)),
                "System.Photo.LensModel" => SetText(ref _lensModel, ReadString(ref value)),
                "System.SoftwareUsed" => SetPreferredSoftware(ReadString(ref value)),
                "System.ApplicationName" => SetText(ref _software, ReadString(ref value)),
                "System.Photo.EXIFVersion" => SetText(ref _exifVersion, ReadString(ref value)),
                "System.Photo.CameraSerialNumber" =>
                    SetText(ref _cameraSerial, ReadString(ref value)),
                "System.Image.CompressionText" =>
                    SetText(ref _compression, ReadString(ref value)),
                "System.Photo.PhotometricInterpretationText" =>
                    SetText(ref _photometricInterpretation, ReadString(ref value)),
                "System.Image.HorizontalSize" =>
                    SetUInt32(ref _width, ref value, 1_000_000),
                "System.Image.VerticalSize" =>
                    SetUInt32(ref _height, ref value, 1_000_000),
                "System.Photo.Orientation" => SetUInt16(ref _orientation, ref value),
                "System.Photo.ISOSpeed" => SetUInt32(ref _iso, ref value, uint.MaxValue),
                "System.Photo.FocalLengthInFilm" =>
                    SetUInt32(ref _focalLengthIn35mmFilm, ref value, uint.MaxValue),
                "System.Photo.ExposureProgram" =>
                    SetUInt16(ref _exposureProgram, ref value),
                "System.Photo.MeteringMode" => SetUInt16(ref _meteringMode, ref value),
                "System.Photo.Flash" => SetUInt16(ref _flash, ref value),
                "System.Photo.WhiteBalance" => SetUInt16(ref _whiteBalance, ref value),
                "System.Photo.LightSource" => SetUInt16(ref _lightSource, ref value),
                "System.Photo.Contrast" => SetUInt16(ref _contrast, ref value),
                "System.Photo.Saturation" => SetUInt16(ref _saturation, ref value),
                "System.Photo.Sharpness" => SetUInt16(ref _sharpness, ref value),
                "System.Image.ColorSpace" => SetUInt16(ref _colorSpace, ref value),
                "System.Image.BitDepth" => SetByte(ref _bitDepth, ref value),
                "System.Photo.FNumber" => SetDouble(ref _fNumber, ref value, 0, 1024),
                "System.Photo.MaxAperture" =>
                    SetDouble(ref _maxAperture, ref value, 0, 1024),
                "System.Photo.ExposureTime" =>
                    SetDouble(ref _exposureTime, ref value, 0, 86_400),
                "System.Photo.FocalLength" =>
                    SetDouble(ref _focalLength, ref value, 0, 1_000_000),
                "System.Photo.ExposureBias" =>
                    SetDouble(
                        ref _exposureBias,
                        ref value,
                        -100,
                        100,
                        includeMinimum: true),
                "System.Photo.DigitalZoom" =>
                    SetDouble(ref _digitalZoomRatio, ref value, 0, 1_000_000),
                "System.Photo.SubjectDistance" =>
                    SetDouble(ref _subjectDistance, ref value, 0, 1_000_000_000),
                "System.Photo.GainControl" => SetIntegerDouble(ref _gainControl, ref value),
                "System.GPS.LatitudeDecimal" =>
                    SetDouble(
                        ref _latitude,
                        ref value,
                        -90,
                        90,
                        includeMinimum: true),
                "System.GPS.LongitudeDecimal" =>
                    SetDouble(
                        ref _longitude,
                        ref value,
                        -180,
                        180,
                        includeMinimum: true),
                "System.GPS.Altitude" =>
                    SetDouble(
                        ref _altitude,
                        ref value,
                        -100_000,
                        100_000,
                        includeMinimum: true),
                "System.GPS.ImgDirection" =>
                    SetDouble(
                        ref _direction,
                        ref value,
                        0,
                        360,
                        includeMinimum: true),
                "System.Image.HorizontalResolution" =>
                    SetDouble(ref _horizontalResolution, ref value, 0, 1_000_000),
                "System.Image.VerticalResolution" =>
                    SetDouble(ref _verticalResolution, ref value, 0, 1_000_000),
                "System.Media.FrameCount" => SetFrameCount(ref value),
                "System.Media.Duration" => SetDuration(ref value),
                _ => false,
            };
            if (accepted)
                Accepted++;
        }

        public ImageMetadata? Build()
        {
            if (Accepted == 0)
                return null;
            return new ImageMetadata
            {
                Title = _title,
                Comment = _comment,
                Make = _make,
                Model = _model,
                DateTime = _dateTime,
                Width = _width,
                Height = _height,
                Orientation = _orientation,
                LensMake = _lensMake,
                LensModel = _lensModel,
                Software = _software,
                FNumber = _fNumber,
                MaxAperture = _maxAperture,
                ExposureTime = _exposureTime,
                Iso = _iso,
                FocalLength = _focalLength,
                FocalLengthIn35mmFilm = _focalLengthIn35mmFilm,
                ExposureBias = _exposureBias,
                ExposureProgram = _exposureProgram,
                MeteringMode = _meteringMode,
                Flash = _flash,
                WhiteBalance = _whiteBalance,
                LightSource = _lightSource,
                DigitalZoomRatio = _digitalZoomRatio,
                SubjectDistance = _subjectDistance,
                Contrast = _contrast,
                Saturation = _saturation,
                Sharpness = _sharpness,
                GainControl = _gainControl,
                ColorSpace = _colorSpace,
                ExifVersion = _exifVersion,
                CameraSerial = _cameraSerial,
                Latitude = _latitude,
                Longitude = _longitude,
                Altitude = _altitude,
                Direction = _direction,
                HorizontalResolution = _horizontalResolution,
                VerticalResolution = _verticalResolution,
                PhotometricInterpretation = _photometricInterpretation,
                BitDepth = _bitDepth,
                Compression = _compression,
                Animated = _animated,
                FrameCount = _frameCount,
                DurationMs = _durationMs,
            };
        }

        private bool SetText(ref string? field, string? value)
        {
            if (field is not null
                || value is null
                || _aggregateStringChars + value.Length > MaxAggregateStringChars)
            {
                return false;
            }
            field = value;
            _aggregateStringChars += value.Length;
            return true;
        }

        private bool SetPreferredSoftware(string? value)
        {
            if (value is null)
                return false;
            int previousLength = _software?.Length ?? 0;
            if (_aggregateStringChars - previousLength + value.Length
                > MaxAggregateStringChars)
            {
                return false;
            }
            _aggregateStringChars -= previousLength;
            _software = value;
            _aggregateStringChars += value.Length;
            return true;
        }

        private static bool SetUInt32(
            ref uint? field,
            ref PropVariant value,
            uint maximum)
        {
            if (field is not null
                || !TryReadUnsigned(ref value, out ulong number)
                || number == 0
                || number > maximum)
            {
                return false;
            }
            field = (uint)number;
            return true;
        }

        private static bool SetUInt16(ref ushort? field, ref PropVariant value)
        {
            if (field is not null
                || !TryReadUnsigned(ref value, out ulong number)
                || number > ushort.MaxValue)
            {
                return false;
            }
            field = (ushort)number;
            return true;
        }

        private static bool SetByte(ref byte? field, ref PropVariant value)
        {
            if (field is not null
                || !TryReadUnsigned(ref value, out ulong number)
                || number == 0
                || number > byte.MaxValue)
            {
                return false;
            }
            field = (byte)number;
            return true;
        }

        private static bool SetDouble(
            ref double? field,
            ref PropVariant value,
            double minimum,
            double maximumInclusive,
            bool includeMinimum = false)
        {
            if (field is not null
                || !TryReadDouble(ref value, out double number)
                || (includeMinimum
                    ? number < minimum
                    : number <= minimum)
                || number > maximumInclusive)
            {
                return false;
            }
            field = number;
            return true;
        }

        private static bool SetIntegerDouble(ref ushort? field, ref PropVariant value)
        {
            if (field is not null
                || !TryReadDouble(ref value, out double number)
                || number < 0
                || number > ushort.MaxValue
                || number != Math.Truncate(number))
            {
                return false;
            }
            field = (ushort)number;
            return true;
        }

        private bool SetFrameCount(ref PropVariant value)
        {
            if (_frameCount is not null
                || !TryReadUnsigned(ref value, out ulong number)
                || number == 0
                || number > uint.MaxValue)
            {
                return false;
            }
            _frameCount = (uint)number;
            _animated = number > 1;
            return true;
        }

        private bool SetDuration(ref PropVariant value)
        {
            if (_durationMs is not null
                || !TryReadUnsigned(ref value, out ulong units100Nanoseconds))
            {
                return false;
            }
            ulong milliseconds = units100Nanoseconds / 10_000;
            if (milliseconds == 0 || milliseconds > uint.MaxValue)
                return false;
            _durationMs = (uint)milliseconds;
            return true;
        }
    }

    private readonly record struct HandlerRegistration(Guid Clsid, string ModulePath);

    private static class PropertyHandlerResolver
    {
        private const string PhotoMetadataHandlerModule = "PhotoMetadataHandler.dll";
        private static readonly Guid PhotoMetadataHandlerClsid =
            new("a38b883c-1682-497e-97b0-0a3a9e801682");

        public static HandlerRegistration? TryResolve(string logicalName)
        {
            try
            {
                string fileName = Path.GetFileName(logicalName);
                if (!string.Equals(fileName, logicalName, StringComparison.Ordinal)
                    || Encoding.UTF8.GetByteCount(fileName) is 0
                        or > NativeAbi.MaxLogicalNameUtf8Bytes)
                {
                    return null;
                }

                string extension = Path.GetExtension(fileName).ToLowerInvariant();
                if (!AllowedExtensions.Contains(extension)
                    || extension.Length is < 2 or > 17
                    || extension[0] != '.'
                    || extension.AsSpan(1).ContainsAnyExcept(
                        "abcdefghijklmnopqrstuvwxyz0123456789"))
                {
                    return null;
                }

                string systemDirectory = Path.GetFullPath(Environment.SystemDirectory);
                if (!Path.IsPathFullyQualified(systemDirectory)
                    || systemDirectory.StartsWith(@"\\", StringComparison.Ordinal))
                {
                    return null;
                }
                string modulePath = Path.GetFullPath(
                    Path.Combine(systemDirectory, PhotoMetadataHandlerModule));
                string expectedModulePath = Path.Combine(
                    systemDirectory,
                    PhotoMetadataHandlerModule);
                if (!string.Equals(
                    modulePath,
                    expectedModulePath,
                    StringComparison.OrdinalIgnoreCase))
                {
                    return null;
                }

                return new HandlerRegistration(PhotoMetadataHandlerClsid, modulePath);
            }
            catch
            {
                return null;
            }
        }
    }

    [ComVisible(true)]
    [ClassInterface(ClassInterfaceType.None)]
    internal sealed class ReadOnlyComStream(
        Stream stream,
        long length,
        CancellationToken cancellationToken) : IRawComStream, IDisposable
    {
        internal const uint MaxSingleReadBytes = 1024 * 1024;
        private const long MaxTotalReadBytes = 32L * 1024 * 1024;
        private const int MaxCalls = 4096;
        private readonly object _gate = new();
        private long _remainingReadBytes = MaxTotalReadBytes;
        private int _calls;
        private bool _disposed;

        public unsafe int Read(nint buffer, uint count, nint bytesRead)
        {
            WriteUInt32(bytesRead, 0);
            lock (_gate)
            {
                int status = CheckOperation();
                if (status < 0)
                    return status;
                if (count > MaxSingleReadBytes || (buffer == 0 && count != 0))
                    return PropertyNative.StgEInvalidPointer;
                if (count == 0)
                    return PropertyNative.SOk;
                if (_remainingReadBytes <= 0)
                    return PropertyNative.StgEReadFault;

                int allowed = (int)Math.Min(count, (ulong)_remainingReadBytes);
                try
                {
                    int read = stream.Read(new Span<byte>((void*)buffer, allowed));
                    _remainingReadBytes -= read;
                    WriteUInt32(bytesRead, checked((uint)read));
                    return PropertyNative.SOk;
                }
                catch (OperationCanceledException)
                {
                    return PropertyNative.HResultCancelled;
                }
                catch (ObjectDisposedException)
                {
                    return PropertyNative.StgEInvalidHandle;
                }
                catch
                {
                    return PropertyNative.StgEReadFault;
                }
            }
        }

        public int Write(nint buffer, uint count, nint bytesWritten)
        {
            WriteUInt32(bytesWritten, 0);
            lock (_gate)
            {
                int status = CheckOperation();
                return status < 0 ? status : PropertyNative.StgEAccessDenied;
            }
        }

        public int Seek(long offset, uint origin, nint newPosition)
        {
            WriteUInt64(newPosition, 0);
            lock (_gate)
            {
                int status = CheckOperation();
                if (status < 0)
                    return status;
                long basis = origin switch
                {
                    0 => 0,
                    1 => stream.Position,
                    2 => length,
                    _ => long.MinValue,
                };
                if (basis == long.MinValue)
                    return PropertyNative.StgEInvalidFunction;
                long target;
                try { target = checked(basis + offset); }
                catch (OverflowException)
                {
                    return PropertyNative.StgESeekError;
                }
                if (target < 0 || target > length)
                    return PropertyNative.StgESeekError;
                try
                {
                    stream.Position = target;
                    WriteUInt64(newPosition, checked((ulong)target));
                    return PropertyNative.SOk;
                }
                catch (ObjectDisposedException)
                {
                    return PropertyNative.StgEInvalidHandle;
                }
                catch
                {
                    return PropertyNative.StgESeekError;
                }
            }
        }

        public int SetSize(ulong newSize)
        {
            lock (_gate)
            {
                int status = CheckOperation();
                return status < 0 ? status : PropertyNative.StgEAccessDenied;
            }
        }

        public unsafe int CopyTo(
            IRawComStream? destination,
            ulong count,
            nint bytesRead,
            nint bytesWritten)
        {
            WriteUInt64(bytesRead, 0);
            WriteUInt64(bytesWritten, 0);
            if (destination is null)
                return PropertyNative.StgEInvalidPointer;

            ulong totalRead = 0;
            ulong totalWritten = 0;
            byte[] buffer = new byte[64 * 1024];
            int result = PropertyNative.SOk;
            try
            {
                while (totalRead < count)
                {
                    int read;
                    lock (_gate)
                    {
                        result = CheckOperation();
                        if (result < 0)
                            break;
                        if (_remainingReadBytes <= 0)
                            break;
                        int requested = (int)Math.Min(
                            (ulong)buffer.Length,
                            Math.Min(count - totalRead, (ulong)_remainingReadBytes));
                        try
                        {
                            read = stream.Read(buffer.AsSpan(0, requested));
                        }
                        catch (OperationCanceledException)
                        {
                            result = PropertyNative.HResultCancelled;
                            break;
                        }
                        catch (ObjectDisposedException)
                        {
                            result = PropertyNative.StgEInvalidHandle;
                            break;
                        }
                        catch
                        {
                            result = PropertyNative.StgEReadFault;
                            break;
                        }
                        _remainingReadBytes -= read;
                    }
                    if (read == 0)
                        break;

                    uint written = 0;
                    fixed (byte* bufferPointer = buffer)
                    {
                        result = destination.Write(
                            (nint)bufferPointer,
                            checked((uint)read),
                            (nint)(&written));
                    }
                    if (result < 0)
                        break;
                    if (written > read)
                    {
                        result = PropertyNative.StgEWriteFault;
                        break;
                    }
                    totalRead += checked((uint)read);
                    totalWritten += written;
                    if (written != read)
                        break;
                }
            }
            finally
            {
                WriteUInt64(bytesRead, totalRead);
                WriteUInt64(bytesWritten, totalWritten);
            }
            return result;
        }

        public int Commit(uint flags)
        {
            lock (_gate)
            {
                int status = CheckOperation();
                return status < 0 ? status : PropertyNative.StgEAccessDenied;
            }
        }

        public int Revert()
        {
            lock (_gate)
            {
                int status = CheckOperation();
                return status < 0 ? status : PropertyNative.StgEInvalidFunction;
            }
        }

        public int LockRegion(ulong offset, ulong count, uint lockType)
        {
            lock (_gate)
            {
                int status = CheckOperation();
                return status < 0 ? status : PropertyNative.StgEInvalidFunction;
            }
        }

        public int UnlockRegion(ulong offset, ulong count, uint lockType)
        {
            lock (_gate)
            {
                int status = CheckOperation();
                return status < 0 ? status : PropertyNative.StgEInvalidFunction;
            }
        }

        public int Stat(out STATSTG stat, uint flags)
        {
            stat = default;
            lock (_gate)
            {
                int status = CheckOperation();
                if (status < 0)
                    return status;
                stat = new STATSTG
                {
                    pwcsName = string.Empty,
                    type = 2,
                    cbSize = length,
                    grfMode = (int)PropertyNative.StgmRead,
                };
                return PropertyNative.SOk;
            }
        }

        public int Clone(out IRawComStream? streamClone)
        {
            streamClone = null;
            lock (_gate)
            {
                int status = CheckOperation();
                return status < 0 ? status : PropertyNative.ENotImpl;
            }
        }

        public void Dispose()
        {
            lock (_gate)
                _disposed = true;
        }

        private int CheckOperation()
        {
            if (_disposed)
                return PropertyNative.StgEInvalidHandle;
            if (cancellationToken.IsCancellationRequested)
                return PropertyNative.HResultCancelled;
            if (++_calls > MaxCalls)
                return PropertyNative.StgEReadFault;
            return PropertyNative.SOk;
        }

        private static void WriteUInt32(nint pointer, uint value)
        {
            if (pointer != 0)
                Marshal.WriteInt32(pointer, unchecked((int)value));
        }

        private static void WriteUInt64(nint pointer, ulong value)
        {
            if (pointer != 0)
                Marshal.WriteInt64(pointer, unchecked((long)value));
        }
    }

    [ComVisible(true)]
    [Guid("0000000C-0000-0000-C000-000000000046")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IRawComStream
    {
        [PreserveSig]
        int Read(nint buffer, uint count, nint bytesRead);

        [PreserveSig]
        int Write(nint buffer, uint count, nint bytesWritten);

        [PreserveSig]
        int Seek(long offset, uint origin, nint newPosition);

        [PreserveSig]
        int SetSize(ulong newSize);

        [PreserveSig]
        int CopyTo(
            [MarshalAs(UnmanagedType.Interface)] IRawComStream? destination,
            ulong count,
            nint bytesRead,
            nint bytesWritten);

        [PreserveSig]
        int Commit(uint flags);

        [PreserveSig]
        int Revert();

        [PreserveSig]
        int LockRegion(ulong offset, ulong count, uint lockType);

        [PreserveSig]
        int UnlockRegion(ulong offset, ulong count, uint lockType);

        [PreserveSig]
        int Stat(out STATSTG stat, uint flags);

        [PreserveSig]
        int Clone([MarshalAs(UnmanagedType.Interface)] out IRawComStream? streamClone);
    }

    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int DllGetClassObjectDelegate(
        [In] ref Guid classId,
        [In] ref Guid interfaceId,
        out nint instance);

    [ComImport]
    [Guid("00000001-0000-0000-C000-000000000046")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IClassFactory
    {
        [PreserveSig]
        int CreateInstance(
            nint outer,
            [In] ref Guid interfaceId,
            out nint instance);

        [PreserveSig]
        int LockServer([MarshalAs(UnmanagedType.Bool)] bool @lock);
    }

    [ComImport]
    [Guid("B824B49D-22AC-4161-AC8A-9916E8FA3F7F")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IInitializeWithStream
    {
        [PreserveSig]
        int Initialize(
            [In, MarshalAs(UnmanagedType.Interface)] IRawComStream stream,
            uint mode);
    }

    [ComImport]
    [Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore
    {
        [PreserveSig]
        int GetCount(out uint count);

        [PreserveSig]
        int GetAt(uint index, out PropertyKey key);

        [PreserveSig]
        int GetValue([In] ref PropertyKey key, out PropVariant value);

        [PreserveSig]
        int SetValue([In] ref PropertyKey key, [In] ref PropVariant value);

        [PreserveSig]
        int Commit();
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PropertyKey
    {
        internal Guid FormatId;
        internal uint PropertyId;
    }

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    private struct CountedValue
    {
        internal uint Count;
        internal nint Pointer;
    }

    [StructLayout(LayoutKind.Explicit, Pack = 8)]
    private struct PropVariant
    {
        [FieldOffset(0)] internal ushort VariantType;
        [FieldOffset(2)] internal ushort Reserved1;
        [FieldOffset(4)] internal ushort Reserved2;
        [FieldOffset(6)] internal ushort Reserved3;
        [FieldOffset(8)] internal sbyte I1;
        [FieldOffset(8)] internal byte Ui1;
        [FieldOffset(8)] internal short I2;
        [FieldOffset(8)] internal ushort Ui2;
        [FieldOffset(8)] internal int I4;
        [FieldOffset(8)] internal uint Ui4;
        [FieldOffset(8)] internal long I8;
        [FieldOffset(8)] internal ulong Ui8;
        [FieldOffset(8)] internal float R4;
        [FieldOffset(8)] internal double R8;
        [FieldOffset(8)] internal long FileTime;
        [FieldOffset(8)] internal nint Pointer;
        [FieldOffset(8)] internal CountedValue Counted;
    }

    private static class PropertyNative
    {
        internal const uint ClsctxInprocServer = 0x1;
        internal const uint CoinitMultithreaded = 0;
        internal const uint LoadLibrarySearchSystem32 = 0x00000800;
        internal const uint StgmRead = 0;
        internal const int SOk = 0;
        internal const ushort VtI2 = 2;
        internal const ushort VtI4 = 3;
        internal const ushort VtR4 = 4;
        internal const ushort VtR8 = 5;
        internal const ushort VtDate = 7;
        internal const ushort VtBstr = 8;
        internal const ushort VtI1 = 16;
        internal const ushort VtUi1 = 17;
        internal const ushort VtUi2 = 18;
        internal const ushort VtUi4 = 19;
        internal const ushort VtI8 = 20;
        internal const ushort VtUi8 = 21;
        internal const ushort VtLpstr = 30;
        internal const ushort VtLpwstr = 31;
        internal const ushort VtFileTime = 64;
        internal const int ENotImpl = unchecked((int)0x80004001);
        internal const int StgEInvalidFunction = unchecked((int)0x80030001);
        internal const int StgEAccessDenied = unchecked((int)0x80030005);
        internal const int StgEInvalidHandle = unchecked((int)0x80030006);
        internal const int StgEInvalidPointer = unchecked((int)0x80030009);
        internal const int StgESeekError = unchecked((int)0x80030019);
        internal const int StgEReadFault = unchecked((int)0x8003001E);
        internal const int StgEWriteFault = unchecked((int)0x8003001D);
        internal const int HResultCancelled = unchecked((int)0x800704C7);
        internal static readonly Guid IidIUnknown =
            new("00000000-0000-0000-C000-000000000046");
        internal static readonly Guid IidClassFactory =
            new("00000001-0000-0000-C000-000000000046");

        [DllImport("ole32.dll", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        internal static extern int CoInitializeEx(nint reserved, uint concurrencyModel);

        [DllImport("ole32.dll", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        internal static extern void CoUninitialize();

        [DllImport("propsys.dll", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        internal static extern int PSGetNameFromPropertyKey(
            [In] ref PropertyKey key,
            out nint canonicalName);

        [DllImport("ole32.dll", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        internal static extern int PropVariantClear(ref PropVariant value);

        [DllImport("oleaut32.dll", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        internal static extern uint SysStringLen(nint bstr);

        [DllImport("kernel32.dll", EntryPoint = "LoadLibraryExW", SetLastError = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        internal static extern nint LoadLibraryEx(
            [MarshalAs(UnmanagedType.LPWStr)] string fileName,
            nint file,
            uint flags);

        [DllImport("kernel32.dll", EntryPoint = "GetProcAddress", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        internal static extern nint GetProcAddress(
            nint module,
            [MarshalAs(UnmanagedType.LPStr)] string procedureName);

    }
}
