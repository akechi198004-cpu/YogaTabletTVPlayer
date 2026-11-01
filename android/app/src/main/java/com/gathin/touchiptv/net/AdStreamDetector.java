package com.gathin.touchiptv.net;

import java.net.URI;
import java.net.URISyntaxException;
import java.net.URLDecoder;
import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** Recognizes explicit ad resource markers without treating ordinary numeric channel names as ads. */
public final class AdStreamDetector {

    private static final String[] AD_PLAYLIST_PATHS = {
            "/applive/mkt.m3u8",
            "/applive/byt.m3u8",
            "/applive/yss.m3u8"
    };
    private static final Pattern AD_SEGMENT_PATH = Pattern.compile(
            "(?:^|/)(?:mkt|byt|yss)/segment_[^/]+\\.ts$",
            Pattern.CASE_INSENSITIVE);
    private static final Pattern URI_ATTRIBUTE_TAG = Pattern.compile(
            "^#EXT-X-(?:KEY|SESSION-KEY|MAP|MEDIA|I-FRAME-STREAM-INF|RENDITION-REPORT|"
                    + "PRELOAD-HINT|PART|IMAGE-STREAM-INF):",
            Pattern.CASE_INSENSITIVE);
    private static final Pattern URI_ATTRIBUTE = Pattern.compile(
            "(?:^|,)\\s*URI\\s*=\\s*(?:\"([^\"]*)\"|([^,\\s]*))",
            Pattern.CASE_INSENSITIVE);

    private AdStreamDetector() {
    }

    /** Matches explicit playlist, segment, or query markers; numeric channel paths alone do not match. */
    public static String findSignature(String value) {
        return findResourceSignature(parseUri(value));
    }

    /** Inspects only actual resource URI lines and URI attributes, not comments or playlist titles. */
    public static String findManifestSignature(String manifest, String manifestUrl) {
        if (manifest == null || manifest.length() == 0) {
            return null;
        }

        URI baseUri = parseUri(manifestUrl);
        String[] lines = manifest.split("\\r?\\n");
        for (String rawLine : lines) {
            String line = rawLine == null ? "" : rawLine.trim();
            if (line.length() == 0) {
                continue;
            }
            if (line.charAt(0) == '#') {
                if (!URI_ATTRIBUTE_TAG.matcher(line).find()) {
                    continue;
                }
                int colon = line.indexOf(':');
                if (colon < 0) {
                    continue;
                }
                Matcher matcher = URI_ATTRIBUTE.matcher(line.substring(colon + 1));
                while (matcher.find()) {
                    String resource = matcher.group(1) != null ? matcher.group(1) : matcher.group(2);
                    String signature = findResolvedResourceSignature(baseUri, resource);
                    if (signature != null) {
                        return signature;
                    }
                }
                continue;
            }

            String signature = findResolvedResourceSignature(baseUri, line);
            if (signature != null) {
                return signature;
            }
        }
        return null;
    }

    public static boolean isAdUrl(String url) {
        return findSignature(url) != null;
    }

    private static String findResolvedResourceSignature(URI baseUri, String resource) {
        if (resource == null || resource.length() == 0) {
            return null;
        }
        try {
            URI resourceUri = new URI(resource.trim());
            if (baseUri != null) {
                resourceUri = baseUri.resolve(resourceUri);
            }
            return findResourceSignature(resourceUri);
        } catch (URISyntaxException ignored) {
            return null;
        }
    }

    private static URI parseUri(String value) {
        if (value == null || value.length() == 0) {
            return null;
        }
        try {
            return new URI(value.trim());
        } catch (URISyntaxException ignored) {
            return null;
        }
    }

    private static String findResourceSignature(URI uri) {
        if (uri == null || uri.getHost() == null
                || !("http".equalsIgnoreCase(uri.getScheme())
                || "https".equalsIgnoreCase(uri.getScheme()))) {
            return null;
        }

        String path = uri.getPath();
        String lowerPath = path == null ? "" : path.toLowerCase(Locale.US);
        for (String adPath : AD_PLAYLIST_PATHS) {
            if (adPath.equals(lowerPath)) {
                return adPath;
            }
        }
        if (AD_SEGMENT_PATH.matcher(lowerPath).find()) {
            return "ad segment path";
        }
        if (hasExactPathSegment(lowerPath, "appadhw") || hasExactPathSegment(lowerPath, "zjstream2")) {
            return "ad marker path";
        }
        return findAdQuerySignature(uri.getRawQuery());
    }

    private static String findAdQuerySignature(String rawQuery) {
        if (rawQuery == null) {
            return null;
        }
        String[] parameters = rawQuery.split("&");
        for (String parameter : parameters) {
            int equals = parameter.indexOf('=');
            if (equals < 0) {
                continue;
            }
            String key = decodeQueryPart(parameter.substring(0, equals));
            String value = decodeQueryPart(parameter.substring(equals + 1));
            if (("a".equals(key) || "app".equals(key)) && "tvzb0006".equals(value)) {
                return key + "=tvzb0006";
            }
        }
        return null;
    }

    private static String decodeQueryPart(String value) {
        try {
            return URLDecoder.decode(value, "UTF-8");
        } catch (Exception ignored) {
            return value;
        }
    }

    private static boolean hasExactPathSegment(String path, String expected) {
        String[] segments = path.split("/");
        for (String segment : segments) {
            if (expected.equals(segment)) {
                return true;
            }
        }
        return false;
    }
}
