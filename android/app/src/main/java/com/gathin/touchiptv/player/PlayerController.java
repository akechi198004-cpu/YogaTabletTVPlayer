package com.gathin.touchiptv.player;

import android.content.Context;
import android.media.AudioManager;
import android.media.MediaPlayer;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.view.Surface;
import android.view.SurfaceHolder;

import com.gathin.touchiptv.net.Net;
import com.gathin.touchiptv.net.AdStreamDetector;
import com.gathin.touchiptv.ui.VideoSurfaceView;
import com.google.android.exoplayer2.DefaultLoadControl;
import com.google.android.exoplayer2.MediaItem;
import com.google.android.exoplayer2.source.MediaSource;
import com.google.android.exoplayer2.PlaybackException;
import com.google.android.exoplayer2.Player;
import com.google.android.exoplayer2.SimpleExoPlayer;
import com.google.android.exoplayer2.source.hls.HlsMediaSource;
import com.google.android.exoplayer2.source.ProgressiveMediaSource;
import com.google.android.exoplayer2.upstream.DefaultDataSource;
import com.google.android.exoplayer2.upstream.DefaultHttpDataSource;
import com.google.android.exoplayer2.video.VideoSize;

import java.util.Locale;

/**
 * Two playback engines behind one small API.
 *
 * <p>ExoPlayer 2.18.7 is the primary engine: it speaks HLS (including AES-128), plain TS and FLV,
 * and reports errors precisely enough to fall back. The platform MediaPlayer is the fallback: on
 * old vendor firmwares it sometimes plays a stream that ExoPlayer's MediaCodec path rejects, at the
 * cost of much worse error reporting. {@link #MODE_AUTO} chains the two; the user can pin either.
 */
public class PlayerController implements SurfaceHolder.Callback {

    public static final int MODE_AUTO = 0;
    public static final int MODE_EXO = 1;
    public static final int MODE_NATIVE = 2;

    public static final int STATE_IDLE = 0;
    public static final int STATE_PREPARING = 1;
    public static final int STATE_BUFFERING = 2;
    public static final int STATE_PLAYING = 3;
    public static final int STATE_PAUSED = 4;
    public static final int STATE_ERROR = 5;
    public static final int STATE_ENDED = 6;

    private static final long PREPARE_TIMEOUT_MS = 20000L;

    public interface Listener {
        void onStateChanged(int state);

        void onVideoSize(int width, int height);

        void onError(String message);

        void onFallbackToNative();
    }

    private final Context ctx;
    private final Handler main = new Handler(Looper.getMainLooper());

    private Listener listener;
    private int mode = MODE_AUTO;
    private volatile String url;
    private volatile long playbackRequestId;
    private Surface surface;
    private int state = STATE_IDLE;
    private volatile boolean playWhenReady = true;
    private volatile boolean foregroundPlaybackAllowed = true;
    private boolean fallbackUsed;
    private boolean fallbackPending;
    private volatile boolean usingNative;
    private boolean hlsTypeExplicit;
    private boolean progressiveTypeExplicit;
    private boolean alternateTypeTried;
    private float volume = 1f;

    private volatile long adHitRequestId = -1L;
    private volatile String adHitSignature;

    private SimpleExoPlayer exo;
    private Player.Listener exoListener;
    private volatile MediaPlayer nativePlayer;
    private Runnable prepareTimeoutTask;
    private long prepareTimeoutRequestId = -1L;
    private Runnable pendingFallbackTask;

    public PlayerController(Context ctx) {
        this.ctx = ctx.getApplicationContext();
    }

    public void setListener(Listener listener) {
        this.listener = listener;
    }

    public void setMode(int mode) {
        if (this.mode != mode) {
            this.mode = mode;
            if (url != null) {
                open(url);
            }
        }
    }

    public int getMode() {
        return mode;
    }

    public boolean isUsingNativeEngine() {
        return usingNative;
    }

    public void attach(VideoSurfaceView view) {
        view.getHolder().addCallback(this);
    }

    // ------------------------------------------------------------------ playback

    public void open(String url) {
        long requestId = ++playbackRequestId;
        this.url = url;
        this.fallbackUsed = false;
        this.fallbackPending = false;
        this.hlsTypeExplicit = isHlsUrl(url);
        this.progressiveTypeExplicit = isProgressiveUrl(url);
        this.alternateTypeTried = false;
        this.adHitRequestId = -1L;
        this.adHitSignature = null;
        this.playWhenReady = true;
        cancelPrepareTimeout();
        cancelPendingFallback(true);
        releaseEngines();
        surface = currentSurface();
        setState(STATE_PREPARING);
        schedulePrepareTimeout(requestId);
        if (mode == MODE_NATIVE) {
            startNative(requestId);
        } else {
            startExo(requestId, hlsTypeExplicit || !progressiveTypeExplicit);
        }
    }

    public void retry() {
        if (url != null) {
            open(url);
        }
    }

    public String currentUrl() {
        return url;
    }

    /** Request token for UI callbacks that defer a line switch. */
    public long currentRequestId() {
        return playbackRequestId;
    }

    public void togglePlayPause() {
        if (usingNative) {
            final MediaPlayer p = nativePlayer;
            if (p == null) {
                return;
            }
            if (state == STATE_ENDED) {
                try {
                    p.start();
                    playWhenReady = true;
                    setState(STATE_PLAYING);
                } catch (Throwable ignored) {
                }
                return;
            }
            // Use playWhenReady as the intent. MediaPlayer.isPlaying() is false while
            // prepareAsync is still running, which must not turn a pause tap into start().
            if (playWhenReady) {
                playWhenReady = false;
                try {
                    p.pause();
                } catch (Throwable ignored) {
                    // Still preparing; onPrepared will honor the paused intent.
                }
                setState(STATE_PAUSED);
            } else {
                playWhenReady = true;
                try {
                    p.start();
                    setState(STATE_PLAYING);
                } catch (Throwable ignored) {
                    // Still preparing: onPrepared will start because playWhenReady is true.
                    if (state != STATE_PREPARING) {
                        setState(STATE_PREPARING);
                    }
                }
            }
        } else if (exo != null) {
            boolean playing = exo.isPlaying() || exo.getPlayWhenReady();
            exo.setPlayWhenReady(!playing);
            playWhenReady = !playing;
            setState(playing ? STATE_PAUSED : STATE_PLAYING);
        } else if (fallbackPending) {
            // No engine is active while the delayed native fallback is pending.
            if (playWhenReady) {
                playWhenReady = false;
                cancelPendingFallback(false);
                setState(STATE_PAUSED);
            } else {
                playWhenReady = true;
                setState(STATE_PREPARING);
                scheduleNativeFallback(playbackRequestId);
            }
        }
    }

    public boolean isPlaying() {
        if (usingNative) {
            try {
                return nativePlayer != null && nativePlayer.isPlaying();
            } catch (Throwable t) {
                return false;
            }
        }
        return exo != null && exo.isPlaying();
    }

    public void pauseForBackground() {
        foregroundPlaybackAllowed = false;
        cancelPrepareTimeout();
        cancelPendingFallback(false);
        if (usingNative) {
            try {
                if (nativePlayer != null) {
                    nativePlayer.pause();
                }
            } catch (Throwable ignored) {
                // Still preparing; onPrepared checks foregroundPlaybackAllowed before starting.
            }
        } else if (exo != null) {
            exo.setPlayWhenReady(false);
        }
        if (state == STATE_PLAYING || state == STATE_BUFFERING) {
            setState(STATE_PAUSED);
        }
    }

    public void resumeAfterBackground() {
        foregroundPlaybackAllowed = true;
        long requestId = playbackRequestId;
        if (playWhenReady) {
            if (exo != null) {
                exo.setPlayWhenReady(true);
            } else if (nativePlayer != null && state == STATE_PAUSED) {
                try {
                    nativePlayer.start();
                    setState(STATE_PLAYING);
                } catch (Throwable ignored) {
                    // If prepareAsync has not completed yet, onPrepared will honor the intent.
                }
            }
            if (fallbackPending) {
                scheduleNativeFallback(requestId);
            }
            if (state == STATE_PREPARING || state == STATE_BUFFERING) {
                schedulePrepareTimeout(requestId);
            }
        }
    }

    public void setVolume(float v) {
        volume = Math.max(0f, Math.min(1f, v));
        if (exo != null) {
            exo.setVolume(volume);
        }
        if (nativePlayer != null) {
            try {
                nativePlayer.setVolume(volume, volume);
            } catch (Throwable ignored) {
            }
        }
    }

    public void release() {
        ++playbackRequestId;
        foregroundPlaybackAllowed = false;
        playWhenReady = false;
        fallbackPending = false;
        url = null;
        cancelPrepareTimeout();
        cancelPendingFallback(true);
        releaseEngines();
        setState(STATE_IDLE);
    }

    // ------------------------------------------------------------------ exo engine

    private void startExo(final long requestId, boolean useHls) {
        if (!isCurrentRequest(requestId)) {
            return;
        }
        usingNative = false;
        try {
            DefaultHttpDataSource.Factory http = new DefaultHttpDataSource.Factory()
                    .setUserAgent(Net.UA)
                    .setConnectTimeoutMs(15000)
                    .setReadTimeoutMs(15000)
                    .setAllowCrossProtocolRedirects(true);
            AdGuardDataSource.Factory guardedHttp = new AdGuardDataSource.Factory(http,
                    new AdGuardDataSource.AdHitListener() {
                        @Override
                        public void onAdHit(String signature) {
                            recordAdHit(requestId, signature);
                        }
                    }, useHls);
            DefaultDataSource.Factory dataSource = new DefaultDataSource.Factory(ctx, guardedHttp);

            MediaSource mediaSource;
            if (useHls) {
                mediaSource = new HlsMediaSource.Factory(dataSource)
                        .setAllowChunklessPreparation(true)
                        .createMediaSource(MediaItem.fromUri(url));
            } else {
                mediaSource = new ProgressiveMediaSource.Factory(dataSource)
                        .createMediaSource(MediaItem.fromUri(url));
            }

            SimpleExoPlayer p = new SimpleExoPlayer.Builder(ctx)
                    .setLoadControl(new DefaultLoadControl())
                    .build();
            Player.Listener requestListener = createExoListener(p, requestId);
            exo = p;
            exoListener = requestListener;
            p.setVolume(volume);
            p.addListener(requestListener);
            if (surface != null && surface.isValid()) {
                p.setVideoSurface(surface);
            }
            p.setMediaSource(mediaSource);
            p.setPlayWhenReady(playWhenReady && foregroundPlaybackAllowed);
            p.prepare();
        } catch (Throwable t) {
            onEngineFailure("播放器初始化失败：" + t.getClass().getSimpleName(), t, requestId);
        }
    }

    private Player.Listener createExoListener(final SimpleExoPlayer player, final long requestId) {
        return new Player.Listener() {
            private boolean isActive() {
                return isCurrentRequest(requestId) && exo == player;
            }

            @Override
            public void onPlaybackStateChanged(int playbackState) {
                if (!isActive()) {
                    return;
                }
                switch (playbackState) {
                    case Player.STATE_BUFFERING:
                        setState(STATE_BUFFERING);
                        break;
                    case Player.STATE_READY:
                        cancelPrepareTimeout(requestId);
                        alternateTypeTried = true;
                        setState(foregroundPlaybackAllowed && playWhenReady && player.getPlayWhenReady()
                                ? STATE_PLAYING : STATE_PAUSED);
                        break;
                    case Player.STATE_ENDED:
                        setState(STATE_ENDED);
                        break;
                    default:
                        break;
                }
            }

            @Override
            public void onIsPlayingChanged(boolean isPlaying) {
                if (!isActive()) {
                    return;
                }
                if (isPlaying && foregroundPlaybackAllowed && playWhenReady) {
                    cancelPrepareTimeout(requestId);
                    setState(STATE_PLAYING);
                } else if (state == STATE_PLAYING) {
                    setState(STATE_PAUSED);
                }
            }

            @Override
            public void onVideoSizeChanged(VideoSize videoSize) {
                if (isActive() && videoSize.width > 0 && videoSize.height > 0 && listener != null) {
                    listener.onVideoSize(videoSize.width, videoSize.height);
                }
            }

            @Override
            public void onPlayerError(PlaybackException error) {
                if (!isActive()) {
                    return;
                }
                String name;
                try {
                    name = error.getErrorCodeName();
                } catch (Throwable t) {
                    name = String.valueOf(error.errorCode);
                }
                onEngineFailure("播放失败：" + name, error, requestId);
            }
        };
    }

    // ------------------------------------------------------------------ native engine

    private void startNative(final long requestId) {
        if (!isCurrentRequest(requestId)) {
            return;
        }
        if (hasAdHit(requestId)) {
            failAdBlockedRequest(requestId, adHitSignature, 0);
            return;
        }
        String adSignature = AdStreamDetector.findSignature(url);
        if (adSignature != null) {
            recordAdHit(requestId, adSignature);
            onEngineFailure("检测到疑似广告流（" + adSignature + "），已阻止播放",
                    new AdGuardDataSource.AdBlockedException(adSignature, 1), requestId);
            return;
        }
        usingNative = true;
        final MediaPlayer mp = new MediaPlayer();
        nativePlayer = mp;
        try {
            mp.setAudioStreamType(AudioManager.STREAM_MUSIC);
            mp.setVolume(volume, volume);
            mp.setOnPreparedListener(new MediaPlayer.OnPreparedListener() {
                @Override
                public void onPrepared(MediaPlayer m) {
                    if (!isCurrentNative(requestId, m)) {
                        return;
                    }
                    cancelPrepareTimeout(requestId);
                    if (!foregroundPlaybackAllowed || !playWhenReady || hasAdHit(requestId)) {
                        setState(STATE_PAUSED);
                        return;
                    }
                    try {
                        m.start();
                        setState(STATE_PLAYING);
                    } catch (Throwable t) {
                        onEngineFailure("系统播放器启动失败", t, requestId);
                    }
                }
            });
            mp.setOnInfoListener(new MediaPlayer.OnInfoListener() {
                @Override
                public boolean onInfo(MediaPlayer m, int what, int extra) {
                    if (!isCurrentNative(requestId, m)) {
                        return false;
                    }
                    if (what == MediaPlayer.MEDIA_INFO_BUFFERING_START) {
                        setState(STATE_BUFFERING);
                    } else if (what == MediaPlayer.MEDIA_INFO_BUFFERING_END) {
                        setState(foregroundPlaybackAllowed && playWhenReady
                                ? STATE_PLAYING : STATE_PAUSED);
                    } else if (what == MediaPlayer.MEDIA_INFO_VIDEO_RENDERING_START) {
                        cancelPrepareTimeout(requestId);
                    }
                    return false;
                }
            });
            mp.setOnVideoSizeChangedListener(new MediaPlayer.OnVideoSizeChangedListener() {
                @Override
                public void onVideoSizeChanged(MediaPlayer m, int width, int height) {
                    if (isCurrentNative(requestId, m) && width > 0 && height > 0 && listener != null) {
                        listener.onVideoSize(width, height);
                    }
                }
            });
            mp.setOnErrorListener(new MediaPlayer.OnErrorListener() {
                @Override
                public boolean onError(MediaPlayer m, int what, int extra) {
                    if (isCurrentNative(requestId, m)) {
                        onEngineFailure("系统播放器解码失败（" + what + "/" + extra + "）", null, requestId);
                    }
                    return true;
                }
            });
            mp.setOnCompletionListener(new MediaPlayer.OnCompletionListener() {
                @Override
                public void onCompletion(MediaPlayer m) {
                    if (isCurrentNative(requestId, m)) {
                        setState(STATE_ENDED);
                    }
                }
            });
            if (surface != null && surface.isValid()) {
                mp.setSurface(surface);
            }
            mp.setDataSource(url);
            mp.prepareAsync();
        } catch (Throwable t) {
            if (nativePlayer == mp) {
                nativePlayer = null;
            }
            releaseNativeInstance(mp);
            onEngineFailure("系统播放器无法打开该地址：" + t.getClass().getSimpleName(), t, requestId);
        }
    }

    // ------------------------------------------------------------------ failure handling

    private void onEngineFailure(String message, Throwable cause, long requestId) {
        if (!isCurrentRequest(requestId) || state == STATE_IDLE || state == STATE_ERROR) {
            return;
        }
        cancelPrepareTimeout(requestId);
        AdGuardDataSource.AdBlockedException adBlocked = findAdBlockedException(cause);
        if (adBlocked != null) {
            recordAdHit(requestId, adBlocked.signature);
        }
        if (hasAdHit(requestId)) {
            failAdBlockedRequest(requestId, adHitSignature, adBlocked == null ? 0 : adBlocked.attempts);
            return;
        }
        if (!usingNative && mode != MODE_NATIVE && !hlsTypeExplicit && !progressiveTypeExplicit
                && !alternateTypeTried) {
            alternateTypeTried = true;
            releaseEngines();
            setState(STATE_PREPARING);
            schedulePrepareTimeout(requestId);
            startExo(requestId, false);
            return;
        }
        if (!usingNative && mode == MODE_AUTO && !fallbackUsed) {
            fallbackUsed = true;
            releaseEngines();
            fallbackPending = true;
            if (listener != null) {
                listener.onFallbackToNative();
            }
            scheduleNativeFallback(requestId);
            return;
        }
        cancelPendingFallback(true);
        releaseEngines();
        setState(STATE_ERROR);
        if (listener != null) {
            listener.onError(message);
        }
    }

    private void failAdBlockedRequest(long requestId, String signature, int attempts) {
        if (!isCurrentRequest(requestId) || state == STATE_ERROR || state == STATE_IDLE) {
            return;
        }
        cancelPrepareTimeout(requestId);
        cancelPendingFallback(true);
        fallbackPending = false;
        releaseEngines();
        setState(STATE_ERROR);
        if (listener != null) {
            String attemptText = attempts > 1 ? "（共尝试 " + attempts + " 次）" : "";
            listener.onError("检测到疑似广告流（" + (signature == null ? "已知特征" : signature)
                    + "），已拦截" + attemptText + "；请切换线路。");
        }
    }

    private void recordAdHit(final long requestId, final String signature) {
        if (!isCurrentRequest(requestId)) {
            return;
        }
        adHitSignature = signature;
        adHitRequestId = requestId;
        main.post(new Runnable() {
            @Override
            public void run() {
                if (!isCurrentRequest(requestId) || !hasAdHit(requestId)
                        || state == STATE_ERROR || state == STATE_IDLE) {
                    return;
                }
                if (fallbackPending || usingNative) {
                    failAdBlockedRequest(requestId, adHitSignature, 0);
                }
            }
        });
    }

    private boolean hasAdHit(long requestId) {
        return adHitRequestId == requestId;
    }

    private boolean isCurrentRequest(long requestId) {
        return requestId == playbackRequestId && url != null;
    }

    private boolean isCurrentNative(long requestId, MediaPlayer player) {
        return isCurrentRequest(requestId) && usingNative && nativePlayer == player;
    }

    private void schedulePrepareTimeout(final long requestId) {
        cancelPrepareTimeout();
        if (!foregroundPlaybackAllowed || !isCurrentRequest(requestId)) {
            return;
        }
        Runnable task = new Runnable() {
            @Override
            public void run() {
                if (prepareTimeoutRequestId != requestId || !isCurrentRequest(requestId)
                        || !foregroundPlaybackAllowed) {
                    return;
                }
                prepareTimeoutTask = null;
                prepareTimeoutRequestId = -1L;
                if (state == STATE_PREPARING || state == STATE_BUFFERING) {
                    onEngineFailure("连接超时：服务器 20 秒无响应", null, requestId);
                }
            }
        };
        prepareTimeoutTask = task;
        prepareTimeoutRequestId = requestId;
        main.postDelayed(task, PREPARE_TIMEOUT_MS);
    }

    private void cancelPrepareTimeout(long requestId) {
        if (prepareTimeoutRequestId == requestId) {
            cancelPrepareTimeout();
        }
    }

    private void cancelPrepareTimeout() {
        if (prepareTimeoutTask != null) {
            main.removeCallbacks(prepareTimeoutTask);
            prepareTimeoutTask = null;
        }
        prepareTimeoutRequestId = -1L;
    }

    private void scheduleNativeFallback(final long requestId) {
        if (!fallbackPending || pendingFallbackTask != null || !isCurrentRequest(requestId)
                || !foregroundPlaybackAllowed || !playWhenReady) {
            return;
        }
        Runnable task = new Runnable() {
            @Override
            public void run() {
                if (pendingFallbackTask != this) {
                    return;
                }
                pendingFallbackTask = null;
                if (!isCurrentRequest(requestId) || !fallbackPending) {
                    return;
                }
                if (hasAdHit(requestId)) {
                    failAdBlockedRequest(requestId, adHitSignature, 0);
                    return;
                }
                if (!foregroundPlaybackAllowed || !playWhenReady) {
                    return;
                }
                fallbackPending = false;
                setState(STATE_PREPARING);
                schedulePrepareTimeout(requestId);
                startNative(requestId);
            }
        };
        pendingFallbackTask = task;
        main.postDelayed(task, 350L);
    }

    private void cancelPendingFallback(boolean clearPending) {
        if (pendingFallbackTask != null) {
            main.removeCallbacks(pendingFallbackTask);
            pendingFallbackTask = null;
        }
        if (clearPending) {
            fallbackPending = false;
        }
    }

    private static boolean isHlsUrl(String value) {
        String path = getUrlPath(value);
        return path.endsWith(".m3u8") || path.endsWith(".m3u");
    }

    private static boolean isProgressiveUrl(String value) {
        String path = getUrlPath(value);
        return path.endsWith(".ts") || path.endsWith(".flv")
                || path.endsWith(".mp4") || path.endsWith(".m4v") || path.endsWith(".m4a")
                || path.endsWith(".mp3") || path.endsWith(".aac") || path.endsWith(".mpg")
                || path.endsWith(".mpeg") || path.endsWith(".mov") || path.endsWith(".avi")
                || path.endsWith(".mkv") || path.endsWith(".webm") || path.endsWith(".3gp");
    }

    private static String getUrlPath(String value) {
        if (value == null) {
            return "";
        }
        try {
            String path = Uri.parse(value).getPath();
            return path == null ? "" : path.toLowerCase(Locale.US);
        } catch (RuntimeException ignored) {
            return "";
        }
    }

    private static AdGuardDataSource.AdBlockedException findAdBlockedException(Throwable cause) {
        Throwable current = cause;
        int depth = 0;
        while (current != null && depth++ < 32) {
            if (current instanceof AdGuardDataSource.AdBlockedException) {
                return (AdGuardDataSource.AdBlockedException) current;
            }
            current = current.getCause();
        }
        return null;
    }

    private void releaseEngines() {
        if (exo != null) {
            final SimpleExoPlayer p = exo;
            final Player.Listener oldListener = exoListener;
            exo = null;
            exoListener = null;
            if (oldListener != null) {
                try {
                    p.removeListener(oldListener);
                } catch (Throwable ignored) {
                }
            }
            try {
                p.stop();
            } catch (Throwable ignored) {
            }
            try {
                p.release();
            } catch (Throwable ignored) {
            }
        }
        if (nativePlayer != null) {
            final MediaPlayer p = nativePlayer;
            nativePlayer = null;
            releaseNativeInstance(p);
        }
        usingNative = false;
    }

    private static void releaseNativeInstance(MediaPlayer player) {
        if (player == null) {
            return;
        }
        try {
            player.setOnPreparedListener(null);
        } catch (Throwable ignored) {
        }
        try {
            player.setOnErrorListener(null);
        } catch (Throwable ignored) {
        }
        try {
            player.setOnInfoListener(null);
        } catch (Throwable ignored) {
        }
        try {
            player.setOnCompletionListener(null);
        } catch (Throwable ignored) {
        }
        try {
            player.setOnVideoSizeChangedListener(null);
        } catch (Throwable ignored) {
        }
        try {
            player.reset();
        } catch (Throwable ignored) {
        }
        try {
            player.release();
        } catch (Throwable ignored) {
        }
    }

    private void setState(int newState) {
        if (state == newState) {
            return;
        }
        state = newState;
        if (listener != null) {
            listener.onStateChanged(newState);
        }
    }

    public int getState() {
        return state;
    }

    private Surface currentSurface() {
        if (surface != null && surface.isValid()) {
            return surface;
        }
        return null;
    }

    // ------------------------------------------------------------------ surface callbacks

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        surface = holder.getSurface();
        if (exo != null) {
            exo.setVideoSurface(surface);
        }
        if (nativePlayer != null) {
            try {
                nativePlayer.setSurface(surface);
            } catch (Throwable ignored) {
            }
        }
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        surface = holder.getSurface();
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        surface = null;
        if (exo != null) {
            exo.clearVideoSurface();
        }
        if (nativePlayer != null) {
            try {
                nativePlayer.setSurface(null);
            } catch (Throwable ignored) {
            }
        }
    }
}
