using System.Runtime.InteropServices;
using Windows.Win32;
using Windows.Win32.Foundation;
using Windows.Win32.Graphics.Direct3D;
using Windows.Win32.Graphics.Direct3D11;
using Windows.Win32.Graphics.Dxgi;
using Windows.Win32.Graphics.Dxgi.Common;

namespace QuickLook.Next.RasterHost;

/// <summary>
/// Producer side of the cross-process composition boundary (validated in Spike 1). Creates a shareable
/// DirectComposition surface handle backed by a composition swap chain. The App copies the handle from
/// this process and composes it into its WinUI 3 visual tree; the OS
/// compositor pulls presented frames on vsync — no per-frame IPC.
///
/// Raster providers hand the host premultiplied BGRA bytes; the host uploads those bytes to a D3D
/// texture, copies that texture into the composition swap chain, and presents exactly one frame.
/// </summary>
internal sealed unsafe class CompositionProducer : IDisposable
{
    private const uint COMPOSITIONOBJECT_ALL_ACCESS = 0x0003;
    private const int MaxPendingSurfaceTransfers = 128;

    private readonly object _sync = new();
    private ID3D11Device _device = null!;
    private ID3D11DeviceContext _ctx = null!;
    private IDXGIFactoryMedia _factory = null!;
    private IDXGISwapChain1? _swapchain;
    private readonly List<IDXGISwapChain1> _liveSwapchains = new();
    private readonly Dictionary<(string RequestId, int PageIndex, long PageGeneration), IDXGISwapChain1> _pageSwapchains = new();
    private readonly List<IDXGISwapChain1> _retired = new(); // closed previews, freed on the next open
    private readonly Dictionary<string, HANDLE> _surfaceTransfers = new(StringComparer.Ordinal);
    private bool _initialized;
    private bool _disposed;
    private int _presentFailureHResult;

    public long AdapterLuid { get; private set; }

    public void Initialize()
    {
        lock (_sync)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            if (_initialized)
                return;

            ReadOnlySpan<D3D_FEATURE_LEVEL> levels = stackalloc[]
            {
                D3D_FEATURE_LEVEL.D3D_FEATURE_LEVEL_11_1,
                D3D_FEATURE_LEVEL.D3D_FEATURE_LEVEL_11_0,
            };
            D3D11_CREATE_DEVICE_FLAG flags = D3D11_CREATE_DEVICE_FLAG.D3D11_CREATE_DEVICE_BGRA_SUPPORT;
#if !DEBUG
            flags |= D3D11_CREATE_DEVICE_FLAG.D3D11_CREATE_DEVICE_PREVENT_ALTERING_LAYER_SETTINGS_FROM_REGISTRY;
#endif
            ID3D11Device? device = null;
            ID3D11DeviceContext? ctx = null;
            IDXGIFactoryMedia? factory = null;
            object? factoryObject = null;
            try
            {
                HRESULT hr = PInvoke.D3D11CreateDevice(
                    null, D3D_DRIVER_TYPE.D3D_DRIVER_TYPE_HARDWARE, default,
                    flags, levels, PInvoke.D3D11_SDK_VERSION,
                    out device, out _, out ctx);
                if (hr.Failed || device is null || ctx is null)
                    throw new InvalidOperationException($"D3D11CreateDevice failed 0x{hr.Value:X8}");

                long adapterLuid = ReadAdapterLuid(device);
                Guid mediaIid = typeof(IDXGIFactoryMedia).GUID;
                hr = PInvoke.CreateDXGIFactory2((DXGI_CREATE_FACTORY_FLAGS)0, &mediaIid, out factoryObject);
                if (hr.Failed || factoryObject is null)
                    throw new InvalidOperationException($"CreateDXGIFactory2 failed 0x{hr.Value:X8}");
                factory = (IDXGIFactoryMedia)factoryObject;

                _device = device;
                _ctx = ctx;
                _factory = factory;
                AdapterLuid = adapterLuid;
                _presentFailureHResult = 0;
                _initialized = true;
            }
            catch
            {
                ReleaseCom(factory ?? factoryObject);
                ReleaseCom(ctx);
                ReleaseCom(device);
                throw;
            }
        }
    }

    private static long ReadAdapterLuid(ID3D11Device device)
    {
        IDXGIDevice dxgiDevice = (IDXGIDevice)device;
        dxgiDevice.GetAdapter(out IDXGIAdapter adapter);
        try
        {
            DXGI_ADAPTER_DESC desc = adapter.GetDesc();
            return ((long)desc.AdapterLuid.HighPart << 32) | desc.AdapterLuid.LowPart;
        }
        finally
        {
            ReleaseCom(adapter);
        }
    }

    public SurfaceTransfer CreateSurface(uint width, uint height)
    {
        lock (_sync)
        {
            EnsureAvailableCore();
            EnsureSurfaceTransferCapacityCore();
            var (surface, sc) = CreateSwapchainCore(width, height);
            SurfaceTransfer transfer = default;
            bool retained = false;
            try
            {
                transfer = RetainSurfaceTransferCore(surface);
                retained = true;
                _liveSwapchains.Add(sc);
                if (_swapchain != null)
                {
                    _retired.Add(_swapchain);
                    _liveSwapchains.Remove(_swapchain);
                    while (_retired.Count > 3)
                    {
                        ReleaseCom(_retired[0]);
                        _retired.RemoveAt(0);
                    }
                }
                _swapchain = sc;
                return transfer;
            }
            catch
            {
                if (retained)
                    ReleaseSurfaceTransferCore(transfer.TransferId);
                else
                    PInvoke.CloseHandle(surface);
                _liveSwapchains.Remove(sc);
                if (ReferenceEquals(_swapchain, sc))
                    _swapchain = null;
                ReleaseCom(sc);
                throw;
            }
        }
    }

    public SurfaceTransfer CreatePresentedSurface(byte[] bgra, int width, int height)
    {
        if (width <= 0 || height <= 0) throw new ArgumentOutOfRangeException(nameof(width));
        int expected = checked(width * height * 4);
        if (bgra.Length != expected)
            throw new ArgumentException($"BGRA buffer length {bgra.Length} does not match {width}x{height}.", nameof(bgra));

        lock (_sync)
        {
            EnsureAvailableCore();
            EnsureSurfaceTransferCapacityCore();
            var (surface, sc) = CreateSwapchainCore((uint)width, (uint)height);
            SurfaceTransfer transfer = default;
            bool retained = false;
            try
            {
                PresentPixelsCore(sc, bgra, width, height);
                transfer = RetainSurfaceTransferCore(surface);
                retained = true;
                _liveSwapchains.Add(sc);
                return transfer;
            }
            catch
            {
                if (retained)
                    ReleaseSurfaceTransferCore(transfer.TransferId);
                else
                    PInvoke.CloseHandle(surface);
                _liveSwapchains.Remove(sc);
                ReleaseCom(sc);
                throw;
            }
        }
    }

    /// <summary>Page surface keyed by request and page, so stale closes cannot release a newer preview.</summary>
    public SurfaceTransfer CreatePresentedPageSurface(
        string requestId, int pageIndex, long pageGeneration, byte[] bgra, int width, int height)
    {
        if (width <= 0 || height <= 0) throw new ArgumentOutOfRangeException(nameof(width));
        int expected = checked(width * height * 4);
        if (bgra.Length != expected)
            throw new ArgumentException($"BGRA buffer length {bgra.Length} does not match {width}x{height}.", nameof(bgra));

        lock (_sync)
        {
            EnsureAvailableCore();
            EnsureSurfaceTransferCapacityCore();
            var (surface, sc) = CreateSwapchainCore((uint)width, (uint)height);
            SurfaceTransfer transfer = default;
            bool retained = false;
            try
            {
                PresentPixelsCore(sc, bgra, width, height);
                transfer = RetainSurfaceTransferCore(surface);
                retained = true;
                var key = (requestId, pageIndex, pageGeneration);
                if (_pageSwapchains.Remove(key, out var old)) ReleaseCom(old);
                _pageSwapchains[key] = sc;
                return transfer;
            }
            catch
            {
                if (retained)
                    ReleaseSurfaceTransferCore(transfer.TransferId);
                else
                    PInvoke.CloseHandle(surface);
                ReleaseCom(sc);
                throw;
            }
        }
    }

    public void ReleasePage(string requestId, int pageIndex, long pageGeneration)
    {
        lock (_sync)
        {
            if (_pageSwapchains.Remove((requestId, pageIndex, pageGeneration), out var sc)) ReleaseCom(sc);
        }
    }

    private static void ReleaseCom(object? com)
    {
        if (com is null) return;
        if (!Marshal.IsComObject(com)) return;
        try { _ = Marshal.FinalReleaseComObject(com); }
        catch (InvalidComObjectException) { }
    }

    private (HANDLE Surface, IDXGISwapChain1 Swapchain) CreateSwapchainCore(uint width, uint height)
    {
        HANDLE surface;
        HRESULT hr = PInvoke.DCompositionCreateSurfaceHandle(COMPOSITIONOBJECT_ALL_ACCESS, null, &surface);
        if (hr.Failed) throw new InvalidOperationException($"DCompositionCreateSurfaceHandle failed 0x{hr.Value:X8}");

        var desc = new DXGI_SWAP_CHAIN_DESC1
        {
            Width = width,
            Height = height,
            Format = DXGI_FORMAT.DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc = new DXGI_SAMPLE_DESC { Count = 1, Quality = 0 },
            BufferUsage = DXGI_USAGE.DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount = 2,
            Scaling = DXGI_SCALING.DXGI_SCALING_STRETCH,
            SwapEffect = DXGI_SWAP_EFFECT.DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode = DXGI_ALPHA_MODE.DXGI_ALPHA_MODE_PREMULTIPLIED,
        };
        try
        {
            _factory.CreateSwapChainForCompositionSurfaceHandle(_device, surface, &desc, null, out IDXGISwapChain1 sc);
            return (surface, sc);
        }
        catch
        {
            PInvoke.CloseHandle(surface);
            throw;
        }
    }

    private void EnsureSurfaceTransferCapacityCore()
    {
        if (_surfaceTransfers.Count >= MaxPendingSurfaceTransfers)
            throw new InvalidOperationException("Too many unacknowledged surface transfers.");
    }

    private SurfaceTransfer RetainSurfaceTransferCore(HANDLE surface)
    {
        string transferId = Guid.NewGuid().ToString("n");
        _surfaceTransfers.Add(transferId, surface);
        return new SurfaceTransfer(transferId, (long)(nint)surface.Value);
    }

    private void ReleaseSurfaceTransferCore(string transferId)
    {
        if (_surfaceTransfers.Remove(transferId, out HANDLE surface))
            PInvoke.CloseHandle(surface);
    }

    public void ReleaseSurfaceTransfer(string transferId)
    {
        lock (_sync)
            ReleaseSurfaceTransferCore(transferId);
    }

    public void PresentPixels(byte[] bgra, int width, int height)
    {
        if (width <= 0 || height <= 0) throw new ArgumentOutOfRangeException(nameof(width));
        int expected = checked(width * height * 4);
        if (bgra.Length != expected)
            throw new ArgumentException($"BGRA buffer length {bgra.Length} does not match {width}x{height}.", nameof(bgra));

        lock (_sync)
        {
            EnsureAvailableCore();
            var sc = _swapchain ?? throw new InvalidOperationException("Surface has not been created.");
            PresentPixelsCore(sc, bgra, width, height);
        }
    }

    private void PresentPixelsCore(IDXGISwapChain1 sc, byte[] bgra, int width, int height)
    {
        int expected = checked(width * height * 4);
        fixed (byte* p = bgra)
        {
            var desc = new D3D11_TEXTURE2D_DESC
            {
                Width = (uint)width,
                Height = (uint)height,
                MipLevels = 1,
                ArraySize = 1,
                Format = DXGI_FORMAT.DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc = new DXGI_SAMPLE_DESC { Count = 1, Quality = 0 },
                Usage = D3D11_USAGE.D3D11_USAGE_DEFAULT,
                BindFlags = 0,
                CPUAccessFlags = 0,
                MiscFlags = 0,
            };
            var data = new D3D11_SUBRESOURCE_DATA
            {
                pSysMem = p,
                SysMemPitch = (uint)(width * 4),
                SysMemSlicePitch = (uint)expected,
            };

            ID3D11Texture2D? source = null;
            ID3D11Texture2D? backbuffer = null;
            try
            {
                _device.CreateTexture2D(desc, data, out source);
                sc.GetBuffer<ID3D11Texture2D>(0, out backbuffer);
                _ctx.CopyResource((ID3D11Resource)backbuffer, (ID3D11Resource)source);
                ThrowIfPresentFailed(sc.Present(1, 0));
            }
            finally
            {
                // The backbuffer RCW keeps the swapchain alive; always release both per-present objects.
                ReleaseCom(backbuffer);
                ReleaseCom(source);
            }
        }
    }

    public void Clear(float r = 0.08f, float g = 0.08f, float b = 0.09f, float a = 1.0f)
    {
        lock (_sync)
        {
            EnsureAvailableCore();
            var sc = _swapchain;
            if (sc is null) return;
            ID3D11Texture2D? backbuffer = null;
            ID3D11RenderTargetView? rtv = null;
            try
            {
                sc.GetBuffer<ID3D11Texture2D>(0, out backbuffer);
                _device.CreateRenderTargetView(backbuffer, null, out rtv);
                _ctx.ClearRenderTargetView(rtv, new float[] { r, g, b, a });
                ThrowIfPresentFailed(sc.Present(1, 0));
            }
            finally
            {
                ReleaseCom(rtv);
                ReleaseCom(backbuffer);
            }
        }
    }

    private void ThrowIfPresentFailed(HRESULT hr)
    {
        if (hr.Failed)
        {
            _presentFailureHResult = hr.Value;
            throw new COMException($"DXGI Present failed 0x{hr.Value:X8}", hr.Value);
        }
    }

    /// <summary>
    /// Close a preview without freeing its GPU surfaces yet: move them to the retired bucket. The App's
    /// compositor may still be holding the just-presented frame for a beat; freeing the swapchain out from
    /// under it would AV. The retired surfaces are freed by <see cref="ReleaseRetired"/> on the next open,
    /// by which point the App has switched away (a full Close→Open round-trip later).
    /// </summary>
    public void Retire()
    {
        lock (_sync)
        {
            if (_disposed) return;
            foreach (var sc in _pageSwapchains.Values) _retired.Add(sc);
            _pageSwapchains.Clear();
            _retired.AddRange(_liveSwapchains);
            _liveSwapchains.Clear();
            _swapchain = null;
        }
    }

    /// <summary>Free surfaces retired by a previous <see cref="Retire"/>. Called when a new preview opens.</summary>
    public void ReleaseRetired()
    {
        lock (_sync)
        {
            if (_disposed) return;
            foreach (var sc in _retired) ReleaseCom(sc);
            _retired.Clear();
        }
    }

    public void Reset()
    {
        lock (_sync)
        {
            if (_disposed) return;
            ResetCore();
        }
    }

    private void ResetCore()
    {
        foreach (var sc in _retired) ReleaseCom(sc);
        _retired.Clear();
        foreach (var sc in _pageSwapchains.Values) ReleaseCom(sc);
        _pageSwapchains.Clear();
        foreach (var sc in _liveSwapchains) ReleaseCom(sc);
        _swapchain = null;
        _liveSwapchains.Clear();
        foreach (HANDLE surface in _surfaceTransfers.Values) PInvoke.CloseHandle(surface);
        _surfaceTransfers.Clear();
    }

    private void EnsureAvailableCore()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        if (!_initialized)
            throw new InvalidOperationException("Composition producer is not initialized.");
        if (_presentFailureHResult != 0)
            throw new COMException(
                $"Composition producer is unavailable after DXGI Present failed 0x{_presentFailureHResult:X8}.",
                _presentFailureHResult);
    }

    public void Dispose()
    {
        lock (_sync)
        {
            if (_disposed) return;
            _disposed = true;
            if (_initialized)
            {
                try
                {
                    _ctx.ClearState();
                    _ctx.Flush();
                }
                catch (Exception)
                {
                    // Continue releasing the owned graph even if a poisoned device rejects cleanup.
                }
            }
            ResetCore();
            ReleaseCom(_ctx);
            ReleaseCom(_device);
            ReleaseCom(_factory);
            _factory = null!;
            _ctx = null!;
            _device = null!;
            _initialized = false;
            _presentFailureHResult = 0;
        }
    }
}

internal readonly record struct SurfaceTransfer(string TransferId, long HostHandle);
