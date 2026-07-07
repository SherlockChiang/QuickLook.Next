using System.Globalization;

namespace QuickLook.Next.App;

internal readonly record struct MapLocation(double Latitude, double Longitude)
{
    public string ToQueryString()
        => string.Create(
            CultureInfo.InvariantCulture,
            $"{Latitude:0.#####},{Longitude:0.#####}");
}

internal static class ExifMapLocation
{
    public static MapLocation NormalizeForGoogleMaps(double latitude, double longitude)
        => IsInChinaMapOffsetRegion(latitude, longitude)
            ? Wgs84ToGcj02(latitude, longitude)
            : new MapLocation(latitude, longitude);

    public static bool IsInChinaMapOffsetRegion(double latitude, double longitude)
        => latitude is >= 0.8293 and <= 55.8271
            && longitude is >= 72.004 and <= 137.8347;

    public static MapLocation Wgs84ToGcj02(double latitude, double longitude)
    {
        const double a = 6378245.0;
        const double ee = 0.00669342162296594323;
        const double pi = Math.PI;

        double dLat = TransformChinaLatitude(longitude - 105.0, latitude - 35.0);
        double dLon = TransformChinaLongitude(longitude - 105.0, latitude - 35.0);
        double radLat = latitude / 180.0 * pi;
        double magic = Math.Sin(radLat);
        magic = 1 - ee * magic * magic;
        double sqrtMagic = Math.Sqrt(magic);
        dLat = dLat * 180.0 / ((a * (1 - ee)) / (magic * sqrtMagic) * pi);
        dLon = dLon * 180.0 / (a / sqrtMagic * Math.Cos(radLat) * pi);
        return new MapLocation(latitude + dLat, longitude + dLon);
    }

    private static double TransformChinaLatitude(double x, double y)
    {
        double ret = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * Math.Sqrt(Math.Abs(x));
        ret += (20.0 * Math.Sin(6.0 * x * Math.PI) + 20.0 * Math.Sin(2.0 * x * Math.PI)) * 2.0 / 3.0;
        ret += (20.0 * Math.Sin(y * Math.PI) + 40.0 * Math.Sin(y / 3.0 * Math.PI)) * 2.0 / 3.0;
        ret += (160.0 * Math.Sin(y / 12.0 * Math.PI) + 320 * Math.Sin(y * Math.PI / 30.0)) * 2.0 / 3.0;
        return ret;
    }

    private static double TransformChinaLongitude(double x, double y)
    {
        double ret = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * Math.Sqrt(Math.Abs(x));
        ret += (20.0 * Math.Sin(6.0 * x * Math.PI) + 20.0 * Math.Sin(2.0 * x * Math.PI)) * 2.0 / 3.0;
        ret += (20.0 * Math.Sin(x * Math.PI) + 40.0 * Math.Sin(x / 3.0 * Math.PI)) * 2.0 / 3.0;
        ret += (150.0 * Math.Sin(x / 12.0 * Math.PI) + 300.0 * Math.Sin(x / 30.0 * Math.PI)) * 2.0 / 3.0;
        return ret;
    }
}
