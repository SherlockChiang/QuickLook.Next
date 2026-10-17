using QuickLook.Next.Core;
using QuickLook.Next.Contracts;
using QuickLook.Next.App;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class HdrImagePolicyTests
{
    private static readonly HdrImageSourceInfo HdrSource = new(
        ImageColorProfileKind.PqBt2020,
        BitDepth: 10,
        MetadataComplete: true);

    [Fact]
    public void Selects_sdr_for_standard_dynamic_range_display()
    {
        PreviewSurfacePlan plan = HdrImagePolicy.Select(
            DisplayColorCapability.StandardDynamicRange,
            HdrSource,
            fp16SurfaceSupported: true);

        Assert.Equal(PreviewSurfaceFormats.SdrBgra8, plan.Format);
        Assert.False(plan.IsHdr);
    }

    [Fact]
    public void Selects_sdr_for_sdr_source_on_hdr_display()
    {
        PreviewSurfacePlan plan = HdrImagePolicy.Select(
            DisplayColorCapability.HighDynamicRange,
            new HdrImageSourceInfo(ImageColorProfileKind.StandardDynamicRange, 8, true),
            fp16SurfaceSupported: true);

        Assert.Equal(PreviewSurfacePlan.Sdr, plan);
    }

    [Fact]
    public void Selects_fp16_scrgb_only_for_complete_hdr_source()
    {
        PreviewSurfacePlan plan = HdrImagePolicy.Select(
            DisplayColorCapability.HighDynamicRange,
            HdrSource,
            fp16SurfaceSupported: true);

        Assert.Equal(PreviewSurfaceFormats.HdrScRgbFp16, plan.Format);
        Assert.True(plan.IsHdr);
    }

    [Fact]
    public void Falls_back_when_fp16_surface_is_unavailable_or_metadata_is_incomplete()
    {
        Assert.Equal(
            PreviewSurfacePlan.Sdr,
            HdrImagePolicy.Select(DisplayColorCapability.HighDynamicRange, HdrSource, false));
        Assert.Equal(
            PreviewSurfacePlan.Sdr,
            HdrImagePolicy.Select(
                DisplayColorCapability.HighDynamicRange,
                HdrSource with { MetadataComplete = false },
                true));
    }

    [Fact]
    public void Keeps_hlg_metadata_sdr_until_ootf_mapping_is_implemented()
    {
        var hlg = new HdrImageSourceInfo(
            ImageColorProfileKind.HlgBt2020,
            BitDepth: 10,
            MetadataComplete: true);

        Assert.True(hlg.IsHdr);
        Assert.Equal(
            PreviewSurfacePlan.Sdr,
            HdrImagePolicy.Select(
                DisplayColorCapability.HighDynamicRange,
                hlg,
                fp16SurfaceSupported: true));
    }

    [Fact]
    public void Reconcile_rejects_unknown_and_mismatched_surface_formats()
    {
        Assert.Equal(
            PreviewSurfacePlan.Sdr,
            HdrImagePolicy.Reconcile(DisplayColorCapability.HighDynamicRange, HdrSource, "RGBA16_FLOAT"));
        Assert.Equal(
            PreviewSurfacePlan.Sdr,
            HdrImagePolicy.Reconcile(
                DisplayColorCapability.HighDynamicRange,
                new HdrImageSourceInfo(ImageColorProfileKind.StandardDynamicRange, 8, true),
                PreviewSurfaceFormats.HdrScRgbFp16));
        Assert.Equal(
            PreviewSurfacePlan.Hdr,
            HdrImagePolicy.Reconcile(DisplayColorCapability.HighDynamicRange, HdrSource, PreviewSurfaceFormats.HdrScRgbFp16));
    }

    [Fact]
    public void Surface_format_constants_are_wire_stable_and_known()
    {
        Assert.True(PreviewSurfaceFormats.IsKnown("B8G8R8A8_UNORM"));
        Assert.True(PreviewSurfaceFormats.IsKnown("R16G16B16A16_FLOAT_SCRGB"));
        Assert.False(PreviewSurfaceFormats.IsKnown("RGBA16_FLOAT"));
        Assert.True(PreviewSurfaceFormats.IsHdr("R16G16B16A16_FLOAT_SCRGB"));
        Assert.False(PreviewSurfaceFormats.IsHdr("B8G8R8A8_UNORM"));
    }

    [Fact]
    public void Hdr_surface_flag_round_trips_without_changing_legacy_default()
    {
        var hdr = new PreviewSurface(
            "request",
            123,
            1920,
            1080,
            96,
            PreviewSurfaceFormats.HdrScRgbFp16)
        {
            IsHdr = true,
            TransferId = "0".PadLeft(32, '0'),
        };

        string json = ProtocolJson.Serialize(hdr);
        PreviewSurface decoded = Assert.IsType<PreviewSurface>(ProtocolJson.Deserialize(json));

        Assert.Contains("\"isHdr\":true", json, StringComparison.Ordinal);
        Assert.Equal(hdr, decoded);

        PreviewSurface legacy = Assert.IsType<PreviewSurface>(ProtocolJson.Deserialize(
            "{\"type\":\"preview.surface\",\"requestId\":\"request\",\"sharedHandle\":123,\"width\":1,\"height\":1,\"dpi\":96,\"format\":\"B8G8R8A8_UNORM\"}"));
        Assert.Null(legacy.IsHdr);
    }

    [Fact]
    public void Host_ready_reports_optional_fp16_capability()
    {
        var ready = new HostReady(1234) { HdrScRgbSurfaceSupported = true };
        string json = ProtocolJson.Serialize(ready);
        HostReady decoded = Assert.IsType<HostReady>(ProtocolJson.Deserialize(json));

        Assert.Contains("\"hdrScRgbSurfaceSupported\":true", json, StringComparison.Ordinal);
        Assert.True(decoded.HdrScRgbSurfaceSupported);

        HostReady legacy = Assert.IsType<HostReady>(ProtocolJson.Deserialize(
            "{\"type\":\"host.ready\",\"adapterLuid\":1234}"));
        Assert.False(legacy.HdrScRgbSurfaceSupported);
    }

    [Fact]
    public void Display_refresh_state_rejects_stale_surfaces_and_tracks_latest_generation()
    {
        var state = new ImageDisplayRefreshState();
        state.Begin("request", hdrEnabled: false);

        PreviewImageRefresh first = Assert.IsType<PreviewImageRefresh>(state.Request("request", hdrEnabled: true));
        Assert.Equal(1, first.ImageGeneration);
        Assert.False(state.CanAccept(new PreviewSurface(
            "request", 1, 1, 1, 96, PreviewSurfaceFormats.SdrBgra8)
        {
            ImageGeneration = first.ImageGeneration,
        }, hdrEnabled: false));

        var current = new PreviewSurface(
            "request", 1, 1, 1, 96, PreviewSurfaceFormats.HdrScRgbFp16)
        {
            ImageGeneration = first.ImageGeneration,
        };
        Assert.True(state.CanAccept(current, hdrEnabled: true));
        Assert.Null(state.Commit(current));

        PreviewImageRefresh second = Assert.IsType<PreviewImageRefresh>(state.Request("request", hdrEnabled: false));
        Assert.Equal(2, second.ImageGeneration);
        Assert.False(state.CanAccept(current, hdrEnabled: false));
        var latest = new PreviewSurface(
            "request", 2, 1, 1, 96, PreviewSurfaceFormats.SdrBgra8)
        {
            ImageGeneration = second.ImageGeneration,
        };
        Assert.True(state.CanAccept(latest, hdrEnabled: false));
        Assert.Equal(first.ImageGeneration, state.Commit(latest)?.ImageGeneration);
    }

    [Fact]
    public void Display_refresh_messages_round_trip_with_legacy_protocol_options()
    {
        var refresh = new PreviewImageRefresh("request", 7, HdrDisplayEnabled: true);
        ControlMessage decoded = ProtocolJson.Deserialize(ProtocolJson.Serialize(refresh));

        Assert.Equal(refresh, Assert.IsType<PreviewImageRefresh>(decoded));
        Assert.Equal(
            new PreviewImageRefreshClose("request", 7),
            Assert.IsType<PreviewImageRefreshClose>(ProtocolJson.Deserialize(
                "{\"type\":\"preview.image.refresh.close\",\"requestId\":\"request\",\"imageGeneration\":7}")));
    }

    [Fact]
    public void Metadata_mapper_requires_explicit_hdr_transfer_primaries_and_completeness()
    {
        HdrImageSourceInfo pq = HdrImagePolicy.FromMetadata(new ImageMetadata
        {
            BitDepth = 12,
            HdrTransferFunction = "SMPTE ST 2084",
            HdrColorPrimaries = "BT.2020",
            HdrMetadataComplete = true,
        });

        Assert.Equal(ImageColorProfileKind.PqBt2020, pq.Profile);
        Assert.True(pq.IsHdr);

        HdrImageSourceInfo hlg = HdrImagePolicy.FromMetadata(new ImageMetadata
        {
            BitDepth = 10,
            HdrTransferFunction = "ARIB STD-B67",
            HdrColorPrimaries = "BT2020",
            HdrMetadataComplete = true,
        });
        Assert.Equal(ImageColorProfileKind.HlgBt2020, hlg.Profile);
        Assert.True(hlg.IsHdr);

        Assert.False(HdrImagePolicy.FromMetadata(new ImageMetadata
        {
            BitDepth = 16,
            HdrTransferFunction = "PQ",
            HdrColorPrimaries = "BT.2020",
            HdrMetadataComplete = false,
        }).IsHdr);
        Assert.False(HdrImagePolicy.FromMetadata(new ImageMetadata
        {
            BitDepth = 16,
            HdrTransferFunction = "PQ",
            HdrColorPrimaries = "sRGB",
            HdrMetadataComplete = true,
        }).IsHdr);
    }
}
