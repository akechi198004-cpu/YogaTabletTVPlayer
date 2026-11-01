package com.gathin.touchiptv.player;

import android.net.Uri;

import com.gathin.touchiptv.net.AdStreamDetector;
import com.google.android.exoplayer2.C;
import com.google.android.exoplayer2.upstream.DataSource;
import com.google.android.exoplayer2.upstream.DataSpec;
import com.google.android.exoplayer2.upstream.TransferListener;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.charset.Charset;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

/**
 * Screens ExoPlayer HTTP responses for known ad redirects and ad-only HLS manifests.
 *
 * <p>The upstream factory still handles TLS, headers and redirects. This wrapper checks the final
 * URI after open(), and buffers only small HLS manifests so their segment paths can be inspected
 * before any playlist bytes reach the player. Suspected ad responses are closed and retried from
 * the original URI; after six ad responses the request fails closed.
 */
public final class AdGuardDataSource implements DataSource {

    private static final int MAX_ATTEMPTS = 6;
    private static final int MAX_PLAYLIST_SCAN_BYTES = 1024 * 1024;
    private static final int PLAYLIST_READ_CHUNK_BYTES = 8192;
    private static final int HLS_HEADER_PROBE_BYTES = 64;
    private static final Charset UTF8 = Charset.forName("UTF-8");

    public interface AdHitListener {
        void onAdHit(String signature);
    }

    public static final class Factory implements DataSource.Factory {
        private final DataSource.Factory upstreamFactory;
        private final AdHitListener adHitListener;
        private final boolean hlsSource;

        public Factory(DataSource.Factory upstreamFactory) {
            this(upstreamFactory, null, false);
        }

        public Factory(DataSource.Factory upstreamFactory, AdHitListener adHitListener) {
            this(upstreamFactory, adHitListener, false);
        }

        public Factory(DataSource.Factory upstreamFactory, AdHitListener adHitListener, boolean hlsSource) {
            this.upstreamFactory = upstreamFactory;
            this.adHitListener = adHitListener;
            this.hlsSource = hlsSource;
        }

        @Override
        public DataSource createDataSource() {
            return new AdGuardDataSource(upstreamFactory, adHitListener, hlsSource);
        }
    }

    public static final class AdBlockedException extends IOException {
        public final String signature;
        public final int attempts;

        public AdBlockedException(String signature, int attempts) {
            super("AD_BLOCKED: " + signature + " after " + attempts + " attempt(s)");
            this.signature = signature;
            this.attempts = attempts;
        }
    }

    private final DataSource.Factory upstreamFactory;
    private final AdHitListener adHitListener;
    private final boolean hlsSource;
    private final List<TransferListener> transferListeners = new ArrayList<TransferListener>();

    private DataSource upstream;
    private byte[] prefetchedPrefix;
    private int prefetchedPosition;
    private boolean prefetchedEof;

    private AdGuardDataSource(DataSource.Factory upstreamFactory, AdHitListener adHitListener,
                              boolean hlsSource) {
        this.upstreamFactory = upstreamFactory;
        this.adHitListener = adHitListener;
        this.hlsSource = hlsSource;
    }

    @Override
    public void addTransferListener(TransferListener transferListener) {
        if (transferListener == null || transferListeners.contains(transferListener)) {
            return;
        }
        transferListeners.add(transferListener);
        if (upstream != null) {
            upstream.addTransferListener(transferListener);
        }
    }

    @Override
    public long open(DataSpec dataSpec) throws IOException {
        closeCurrentQuietly();
        resetPrefetch();

        String requestedUrl = dataSpec.uri.toString();
        String directSignature = AdStreamDetector.findSignature(requestedUrl);
        if (directSignature != null) {
            notifyAdHit(directSignature);
            throw new AdBlockedException(directSignature, 1);
        }

        for (int attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
            if (attempt > 1) {
                pauseBeforeRetry(attempt - 1);
            }

            DataSource candidate = upstreamFactory.createDataSource();
            for (TransferListener transferListener : transferListeners) {
                candidate.addTransferListener(transferListener);
            }

            try {
                long length = candidate.open(dataSpec);
                String signature = findResponseUrlSignature(candidate);
                byte[] prefix = null;
                boolean reachedEof = false;

                boolean knownPlaylist = isPlaylistRequest(dataSpec, candidate);
                boolean probeHlsBody = hlsSource && isFullResourceRequest(dataSpec)
                        && !isKnownNonPlaylistResource(dataSpec.uri) && !knownPlaylist;
                if (signature == null && (knownPlaylist || probeHlsBody)) {
                    PrefetchResult result = inspectPlaylist(candidate);
                    prefix = result.bytes;
                    reachedEof = result.reachedEof;
                    if (result.isManifest) {
                        Uri manifestUri = candidate.getUri();
                        String manifestUrl = manifestUri == null
                                ? requestedUrl : manifestUri.toString();
                        signature = AdStreamDetector.findManifestSignature(
                                new String(prefix, UTF8), manifestUrl);
                    }
                    if (result.isManifest && signature == null && !reachedEof) {
                        signature = "manifest exceeds inspection limit";
                    }
                }

                if (signature != null) {
                    notifyAdHit(signature);
                    closeQuietly(candidate);
                    if (attempt == MAX_ATTEMPTS) {
                        throw new AdBlockedException(signature, attempt);
                    }
                    continue;
                }

                upstream = candidate;
                if (prefix != null) {
                    prefetchedPrefix = prefix;
                    prefetchedEof = reachedEof;
                }
                return length;
            } catch (AdBlockedException e) {
                throw e;
            } catch (IOException e) {
                closeQuietly(candidate);
                throw e;
            } catch (RuntimeException e) {
                closeQuietly(candidate);
                throw e;
            }
        }

        throw new AdBlockedException("unknown", MAX_ATTEMPTS);
    }

    @Override
    public int read(byte[] buffer, int offset, int length) throws IOException {
        if (length == 0) {
            return 0;
        }
        if (prefetchedPrefix != null && prefetchedPosition < prefetchedPrefix.length) {
            int count = Math.min(length, prefetchedPrefix.length - prefetchedPosition);
            System.arraycopy(prefetchedPrefix, prefetchedPosition, buffer, offset, count);
            prefetchedPosition += count;
            return count;
        }
        if (prefetchedEof) {
            return C.RESULT_END_OF_INPUT;
        }
        if (upstream == null) {
            throw new IOException("Data source is not open");
        }
        return upstream.read(buffer, offset, length);
    }

    @Override
    public Uri getUri() {
        return upstream == null ? null : upstream.getUri();
    }

    @Override
    public Map<String, List<String>> getResponseHeaders() {
        return upstream == null ? Collections.<String, List<String>>emptyMap() : upstream.getResponseHeaders();
    }

    @Override
    public void close() throws IOException {
        DataSource current = upstream;
        upstream = null;
        resetPrefetch();
        if (current != null) {
            current.close();
        }
    }

    private String findResponseUrlSignature(DataSource candidate) {
        Uri finalUri = candidate.getUri();
        String signature = AdStreamDetector.findSignature(finalUri == null ? null : finalUri.toString());
        if (signature != null) {
            return signature;
        }

        for (Map.Entry<String, List<String>> header : candidate.getResponseHeaders().entrySet()) {
            if ("location".equalsIgnoreCase(header.getKey()) && header.getValue() != null) {
                for (String value : header.getValue()) {
                    signature = AdStreamDetector.findSignature(value);
                    if (signature != null) {
                        return signature;
                    }
                }
            }
        }
        return null;
    }

    private void notifyAdHit(String signature) {
        if (adHitListener != null) {
            adHitListener.onAdHit(signature);
        }
    }

    private boolean isPlaylistRequest(DataSpec dataSpec, DataSource candidate) {
        if (!isFullResourceRequest(dataSpec)) {
            return false;
        }
        if (isPlaylistUri(dataSpec.uri)) {
            return true;
        }
        Uri finalUri = candidate.getUri();
        if (isPlaylistUri(finalUri)) {
            return true;
        }
        for (Map.Entry<String, List<String>> header : candidate.getResponseHeaders().entrySet()) {
            if ("content-type".equalsIgnoreCase(header.getKey()) && header.getValue() != null) {
                for (String value : header.getValue()) {
                    String contentType = value == null ? "" : value.toLowerCase(java.util.Locale.US);
                    if (contentType.contains("mpegurl") || contentType.contains("mpeg-url")) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    private static boolean isFullResourceRequest(DataSpec dataSpec) {
        return dataSpec.position == 0 && dataSpec.length == C.LENGTH_UNSET;
    }

    private static boolean isKnownNonPlaylistResource(Uri uri) {
        if (uri == null || uri.getPath() == null) {
            return false;
        }
        String path = uri.getPath().toLowerCase(java.util.Locale.US);
        String[] mediaExtensions = {
                ".ts", ".m4s", ".mp4", ".m4v", ".aac", ".ac3", ".ec3", ".mp3",
                ".flv", ".mkv", ".avi", ".mov", ".webm", ".vtt", ".jpg", ".jpeg",
                ".png", ".webp", ".key", ".bin"
        };
        for (String extension : mediaExtensions) {
            if (path.endsWith(extension)) {
                return true;
            }
        }
        return false;
    }

    private static boolean isPlaylistUri(Uri uri) {
        if (uri == null) {
            return false;
        }
        String path = uri.getPath();
        if (path == null) {
            return false;
        }
        path = path.toLowerCase(java.util.Locale.US);
        return path.endsWith(".m3u8") || path.endsWith(".m3u");
    }

    private PrefetchResult inspectPlaylist(DataSource source) throws IOException {
        ByteArrayOutputStream output = new ByteArrayOutputStream(8192);
        byte[] buffer = new byte[PLAYLIST_READ_CHUNK_BYTES];
        boolean reachedEof = false;

        while (output.size() < HLS_HEADER_PROBE_BYTES) {
            int requestLength = Math.min(buffer.length, HLS_HEADER_PROBE_BYTES - output.size());
            int count = source.read(buffer, 0, requestLength);
            if (count == C.RESULT_END_OF_INPUT) {
                reachedEof = true;
                break;
            }
            if (count > 0) {
                output.write(buffer, 0, count);
            }
        }

        byte[] headerBytes = output.toByteArray();
        boolean isManifest = looksLikeHlsManifest(headerBytes);
        if (!isManifest) {
            return new PrefetchResult(headerBytes, reachedEof, false);
        }

        while (!reachedEof && output.size() <= MAX_PLAYLIST_SCAN_BYTES) {
            int remainingWithOverflowByte = MAX_PLAYLIST_SCAN_BYTES + 1 - output.size();
            int requestLength = Math.min(buffer.length, remainingWithOverflowByte);
            int count = source.read(buffer, 0, requestLength);
            if (count == C.RESULT_END_OF_INPUT) {
                reachedEof = true;
                break;
            }
            if (count > 0) {
                output.write(buffer, 0, count);
            }
        }

        return new PrefetchResult(output.toByteArray(), reachedEof, true);
    }

    private static boolean looksLikeHlsManifest(byte[] bytes) {
        int position = 0;
        if (bytes.length >= 3 && (bytes[0] & 0xff) == 0xef
                && (bytes[1] & 0xff) == 0xbb && (bytes[2] & 0xff) == 0xbf) {
            position = 3;
        }
        while (position < bytes.length) {
            int value = bytes[position] & 0xff;
            if (value == ' ' || value == '\t' || value == '\r' || value == '\n') {
                position++;
            } else {
                break;
            }
        }
        byte[] header = "#EXTM3U".getBytes(UTF8);
        if (bytes.length - position < header.length) {
            return false;
        }
        for (int i = 0; i < header.length; i++) {
            if (bytes[position + i] != header[i]) {
                return false;
            }
        }
        return true;
    }

    private static void pauseBeforeRetry(int retryNumber) throws IOException {
        long delayMs = Math.min(200L, retryNumber * 100L);
        try {
            Thread.sleep(delayMs);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new IOException("Interrupted while retrying an ad-redirected stream", e);
        }
    }

    private void closeCurrentQuietly() {
        DataSource current = upstream;
        upstream = null;
        if (current != null) {
            closeQuietly(current);
        }
    }

    private static void closeQuietly(DataSource source) {
        try {
            source.close();
        } catch (IOException ignored) {
        }
    }

    private void resetPrefetch() {
        prefetchedPrefix = null;
        prefetchedPosition = 0;
        prefetchedEof = false;
    }

    private static final class PrefetchResult {
        final byte[] bytes;
        final boolean reachedEof;
        final boolean isManifest;

        PrefetchResult(byte[] bytes, boolean reachedEof, boolean isManifest) {
            this.bytes = bytes;
            this.reachedEof = reachedEof;
            this.isManifest = isManifest;
        }
    }
}
