using System.Runtime.InteropServices;
using Windows.Win32;
using Windows.Win32.Foundation;
using Windows.Win32.Graphics.Direct3D;
using Windows.Win32.Graphics.Direct3D11;
using Windows.Win32.Graphics.Dxgi;
using Windows.Win32.Graphics.Dxgi.Common;
using Windows.Win32.System.Threading;

namespace QuickLook.Next.RasterHost;

/// <summary>
/// Producer side of the cross-process composition boundary (validated in Spike 1). Creates a shareable
/// DirectComposition surface handle backed by a composition swap chain, duplicates the handle into the
/// App process, and renders into it. The App composes the handle into its WinUI 3 visual tree; the OS
/// compositor pulls presented frames on vsync — no per-frame IPC.
///
/// Raster providers hand the host premultiplied BGRA bytes; the host uploads those bytes to a D3D
/// texture, copies that texture into the composition swap chain, and presents exactly one frame.
/// </summary>
internal sealed unsafe class CompositionProducer : IDisposable
{
    private const uint COMPOSITIONOBJECT_ALL_ACCESS = 0x0003;

    private readonly object _sync = new();
    private ID3D11Device _device = null!;
    private ID3D11DeviceContext _ctx = null!;
    private IDXGIFactoryMedia _factory = null!;
    private IDXGISwapChain1? _swapchain;
    private readonly List<IDXGISwapChain1> _liveSwapchains = new();
    private readonly Dictionary<int, IDXGISwapChain1> _pageSwapchains = new();
    private readonly List<IDXGISwapChain1> _retired = new(); // closed previews, freed on the next open
    private HANDLE _appProc;

    public long AdapterLuid { get; private set; }

    public void Initialize(int appProcessId)
    {
        ReadOnlySpan<D3D_FEATURE_LEVEL> levels = stackalloc[]
        {
            D3D_FEATURE_LEVEL.D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL.D3D_FEATURE_LEVEL_11_0,
        };
        HRESULT hr = PInvoke.D3D11CreateDevice(
            null, D3D_DRIVER_TYPE.D3D_DRIVER_TYPE_HARDWARE, default,
            D3D11_CREATE_DEVICE_FLAG.D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            levels, PInvoke.D3D11_SDK_VERSION,
            out ID3D11Device? device, out _, out ID3D11DeviceContext? ctx);
        if (hr.Failed || device is null || ctx is null)
            throw new InvalidOperationException($"D3D11CreateDevice failed 0x{hr.Value:X8}");
        _device = device;
        _ctx = ctx;
        AdapterLuid = ReadAdapterLuid(_device);

        Guid mediaIid = typeof(IDXGIFactoryMedia).GUID;
        hr = PInvoke.CreateDXGIFactory2((DXGI_CREATE_FACTORY_FLAGS)0, &mediaIid, out object factoryObj);
        if (hr.Failed || factoryObj is null)
            throw new InvalidOperationException($"CreateDXGIFactory2 failed 0x{hr.Value:X8}");
        _factory = (IDXGIFactoryMedia)factoryObj;

        if (appProcessId != 0)
        {
            _appProc = PInvoke.OpenProcess(PROCESS_ACCESS_RIGHTS.PROCESS_DUP_HANDLE, false, (uint)appProcessId);
            if ((nint)_appProc.Value == 0)
                throw new InvalidOperationException("OpenProcess(App) failed");
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

    public long CreateSurface(uint width, uint height)
    {
        var (surface, sc) = CreateSwapchain(width, height);
        lock (_sync)
        {
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
            _liveSwapchains.Add(sc);
        }
        return DuplicateToApp(surface);
    }

    public long CreatePresentedSurface(byte[] bgra, int width, int height)
    {
        if (width <= 0 || height <= 0) throw new ArgumentOutOfRangeException(nameof(width));
        int expected = checked(width * height * 4);
        if (bgra.Length != expected)
            throw new ArgumentException($"BGRA buffer length {bgra.Length} does not match {width}x{height}.", nameof(bgra));

        var (surface, sc) = CreateSwapchain((uint)width, (uint)height);
        lock (_sync)
        {
            _liveSwapchains.Add(sc);
            PresentPixelsCore(sc, bgra, width, height);
        }
        return DuplicateToApp(surface);
    }

    /// <summary>Page surface keyed by page index, so the App can release pages that scroll far away.</summary>
    public long CreatePresentedPageSurface(int pageIndex, byte[] bgra, int width, int height)
    {
        if (width <= 0 || height <= 0) throw new ArgumentOutOfRangeException(nameof(width));
        int expected = checked(width * height * 4);
        if (bgra.Length != expected)
            throw new ArgumentException($"BGRA buffer length {bgra.Length} does not match {width}x{height}.", nameof(bgra));

        var (surface, sc) = CreateSwapchain((uint)width, (uint)height);
        lock (_sync)
        {
            if (_pageSwapchains.Remove(pageIndex, out var old)) ReleaseCom(old);
            _pageSwapchains[pageIndex] = sc;
            PresentPixelsCore(sc, bgra, width, height);
        }
        return DuplicateToApp(surface);
    }

    public void ReleasePage(int pageIndex)
    {
        lock (_sync)
        {
            if (_pageSwapchains.Remove(pageIndex, out var sc)) ReleaseCom(sc);
        }
    }

    private static void ReleaseCom(object? com)
    {
        if (com is null) return;
        try { while (Marshal.ReleaseComObject(com) > 0) { } }
        catch { /* not an RCW; let GC reclaim it */ }
    }

    private (HANDLE Surface, IDXGISwapChain1 Swapchain) CreateSwapchain(uint width, uint height)
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
        _factory.CreateSwapChainForCompositionSurfaceHandle(_device, surface, &desc, null, out IDXGISwapChain1 sc);
        return (surface, sc);
    }

    private long DuplicateToApp(HANDLE surface)
    {
        HANDLE dup;
        BOOL ok = PInvoke.DuplicateHandle(
            PInvoke.GetCurrentProcess(), surface, _appProc, &dup, 0, false,
            DUPLICATE_HANDLE_OPTIONS.DUPLICATE_SAME_ACCESS);
        // The swapchain (created from this handle) holds its own reference to the composition surface, and
        // the App now owns a duplicated handle — so the host's original handle is no longer needed. Close
        // it, or one kernel handle leaks for every surface and every PDF page rendered.
        PInvoke.CloseHandle(surface);
        if (!ok) throw new InvalidOperationException("DuplicateHandle into App failed");
        return (long)(nint)dup.Value;
    }

    public void PresentPixels(byte[] bgra, int width, int height)
    {
        if (width <= 0 || height <= 0) throw new ArgumentOutOfRangeException(nameof(width));
        int expected = checked(width * height * 4);
        if (bgra.Length != expected)
            throw new ArgumentException($"BGRA buffer length {bgra.Length} does not match {width}x{height}.", nameof(bgra));

        lock (_sync)
        {
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

            _device.CreateTexture2D(desc, data, out ID3D11Texture2D source);
            sc.GetBuffer<ID3D11Texture2D>(0, out ID3D11Texture2D backbuffer);
            _ctx.CopyResource((ID3D11Resource)backbuffer, (ID3D11Resource)source);
            sc.Present(1, 0);
            // Release the per-present D3D objects. The backbuffer RCW in particular keeps the swapchain
            // alive, so leaking it means swapchains never fully release even after Retire/Reset — handle
            // and GPU-memory growth on every preview and every PDF page.
            ReleaseCom(backbuffer);
            ReleaseCom(source);
        }
    }

    public void Clear(float r = 0.08f, float g = 0.08f, float b = 0.09f, float a = 1.0f)
    {
        lock (_sync)
        {
            var sc = _swapchain;
            if (sc is null) return;
            sc.GetBuffer<ID3D11Texture2D>(0, out ID3D11Texture2D backbuffer);
            _device.CreateRenderTargetView(backbuffer, null, out ID3D11RenderTargetView rtv);
            _ctx.ClearRenderTargetView(rtv, new float[] { r, g, b, a });
            sc.Present(1, 0);
            ReleaseCom(rtv);
            ReleaseCom(backbuffer);
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
            foreach (var sc in _retired) ReleaseCom(sc);
            _retired.Clear();
        }
    }

    public void Reset()
    {
        lock (_sync)
        {
            foreach (var sc in _retired) ReleaseCom(sc);
            _retired.Clear();
            foreach (var sc in _pageSwapchains.Values) ReleaseCom(sc);
            _pageSwapchains.Clear();
            foreach (var sc in _liveSwapchains) ReleaseCom(sc);
            _swapchain = null;
            _liveSwapchains.Clear();
        }
    }

    public void Dispose()
    {
        Reset();
        if ((nint)_appProc.Value != 0) PInvoke.CloseHandle(_appProc);
        _appProc = default;
        ReleaseCom(_factory);
        ReleaseCom(_ctx);
        ReleaseCom(_device);
        _factory = null!;
        _ctx = null!;
        _device = null!;
    }
}
