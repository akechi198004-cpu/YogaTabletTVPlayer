import React, { useEffect, useRef, useState, useCallback } from 'react';
import Hls from 'hls.js';
import { ChannelItem, formatLatency } from '../utils/m3uParser';
import {
  Play,
  Pause,
  Volume2,
  VolumeX,
  Volume1,
  Maximize2,
  Minimize2,
  Maximize,
  Minimize,
  PictureInPicture2,
  RotateCcw,
  Shuffle,
  Star,
  Zap,
  Tv,
  AlertCircle,
  SkipBack,
  SkipForward,
  Radio,
  Sun,
  Moon,
} from 'lucide-react';

interface PlayerProps {
  channel: ChannelItem | null;
  activeLine: number;
  onLineChange: (line: number) => void;
  onRetry: () => void;
  onToggleFavorite: (name: string) => void;
  isFavorite: boolean;
  isFullscreen: boolean;
  onToggleFullscreen: (forceExit?: boolean) => void;
  isTheaterMode: boolean;
  onToggleTheaterMode: () => void;
  onNextChannel?: () => void;
  onPrevChannel?: () => void;
  playerFrameRef: React.RefObject<HTMLDivElement | null>;
  notice: string;
  onTestSpeed: (channel: ChannelItem) => void;
  speedTesting: boolean;
  liveSpeedResult: { latency?: number; resolution?: string } | null;
  theme: 'iron' | 'light';
  onThemeChange: (theme: 'iron' | 'light') => void;
}

function encodeBase64Url(val: string): string {
  try {
    const bytes = new TextEncoder().encode(val);
    let binary = '';
    bytes.forEach((b) => (binary += String.fromCharCode(b)));
    return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  } catch {
    return encodeURIComponent(val);
  }
}

function viaProxy(url: string): string {
  return '/api/proxy?u=' + encodeURIComponent(encodeBase64Url(url));
}

export const Player: React.FC<PlayerProps> = ({
  channel,
  activeLine,
  onLineChange,
  onRetry,
  onToggleFavorite,
  isFavorite,
  isFullscreen,
  onToggleFullscreen,
  isTheaterMode,
  onToggleTheaterMode,
  onNextChannel,
  onPrevChannel,
  playerFrameRef,
  notice,
  onTestSpeed,
  speedTesting,
  liveSpeedResult,
  theme,
  onThemeChange,
}) => {
  const isLight = theme === 'light';
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const hlsRef = useRef<Hls | null>(null);
  const hideControlsTimerRef = useRef<number | null>(null);

  const [loading, setLoading] = useState<boolean>(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [detectedResolution, setDetectedResolution] = useState<string>('');
  const [isPlaying, setIsPlaying] = useState<boolean>(false);
  const [isMuted, setIsMuted] = useState<boolean>(() => {
    try {
      return localStorage.getItem('touchiptv-muted') === 'true';
    } catch {
      return false;
    }
  });
  const [volume, setVolume] = useState<number>(() => {
    try {
      const v = localStorage.getItem('touchiptv-volume');
      return v !== null ? parseFloat(v) : 1;
    } catch {
      return 1;
    }
  });
  const [showControls, setShowControls] = useState<boolean>(true);
  const [playPulse, setPlayPulse] = useState<boolean>(false);
  const [pulseIcon, setPulseIcon] = useState<'play' | 'pause'>('play');

  const currentUrl = channel?.lines?.[activeLine] || channel?.url || '';

  // Wake and auto-hide controls (YouTube style)
  const wakeControls = useCallback(() => {
    setShowControls(true);
    if (hideControlsTimerRef.current) {
      window.clearTimeout(hideControlsTimerRef.current);
    }
    hideControlsTimerRef.current = window.setTimeout(() => {
      if (isPlaying) {
        setShowControls(false);
      }
    }, 3000);
  }, [isPlaying]);

  useEffect(() => {
    wakeControls();
    return () => {
      if (hideControlsTimerRef.current) {
        window.clearTimeout(hideControlsTimerRef.current);
      }
    };
  }, [wakeControls]);

  // Video playback & HLS integration
  useEffect(() => {
    if (!channel || !currentUrl) {
      if (hlsRef.current) {
        hlsRef.current.destroy();
        hlsRef.current = null;
      }
      if (videoRef.current) {
        videoRef.current.pause();
        videoRef.current.removeAttribute('src');
        videoRef.current.load();
      }
      setLoading(false);
      setIsPlaying(false);
      setErrorMessage(null);
      return;
    }

    setLoading(true);
    setErrorMessage(null);
    setDetectedResolution(channel.resolution || '');

    const video = videoRef.current;
    if (!video) return;

    if (hlsRef.current) {
      hlsRef.current.destroy();
      hlsRef.current = null;
    }

    const proxyStreamUrl = viaProxy(currentUrl);
    const isHlsStream = currentUrl.includes('.m3u8') || currentUrl.includes('m3u8') || proxyStreamUrl.includes('m3u8');

    let mounted = true;

    if (isHlsStream && Hls.isSupported()) {
      const hls = new Hls({
        enableWorker: true,
        lowLatencyMode: false,
        backBufferLength: 30,
        maxBufferLength: 20,
        maxMaxBufferLength: 40,
      });
      hlsRef.current = hls;

      hls.loadSource(proxyStreamUrl);
      hls.attachMedia(video);

      hls.on(Hls.Events.MANIFEST_PARSED, (_event, data) => {
        if (!mounted) return;
        setLoading(false);
        if (data.levels && data.levels.length > 0) {
          const firstLevel = data.levels[0];
          if (firstLevel.height) {
            setDetectedResolution(`${firstLevel.height}P`);
          }
        }
        video.play().catch(() => {
          if (mounted) {
            setIsPlaying(false);
            setLoading(false);
          }
        });
      });

      hls.on(Hls.Events.LEVEL_SWITCHED, (_event, data) => {
        if (hls.levels && hls.levels[data.level]?.height) {
          setDetectedResolution(`${hls.levels[data.level].height}P`);
        }
      });

      let networkRetryCount = 0;
      hls.on(Hls.Events.ERROR, (_event, data) => {
        if (!mounted) return;
        if (data.fatal) {
          switch (data.type) {
            case Hls.ErrorTypes.NETWORK_ERROR:
              networkRetryCount++;
              if (networkRetryCount <= 2) {
                hls.startLoad();
              } else if (channel && channel.lines && channel.lines.length > 1) {
                const nextLine = (activeLine + 1) % channel.lines.length;
                onLineChange(nextLine);
              } else {
                setLoading(false);
                setErrorMessage('直播流遇到网络阻断或源端离线，可尝试切换其他备用线路或重试。');
                hls.destroy();
              }
              break;
            case Hls.ErrorTypes.MEDIA_ERROR:
              hls.recoverMediaError();
              break;
            default:
              setLoading(false);
              setErrorMessage('直播流遇到网络阻断，可尝试切换其他备用线路或重试。');
              hls.destroy();
              break;
          }
        }
      });
    } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
      video.src = proxyStreamUrl;
      video.addEventListener('loadedmetadata', () => {
        if (!mounted) return;
        setLoading(false);
        if (video.videoHeight) {
          setDetectedResolution(`${video.videoHeight}P`);
        }
        video.play().catch(() => {});
      });
      video.addEventListener('error', () => {
        if (!mounted) return;
        setLoading(false);
        setErrorMessage('当前浏览器无法直接解码此流，请切换备用线路。');
      });
    } else {
      video.src = proxyStreamUrl;
      video.play().catch(() => {});
      setLoading(false);
    }

    return () => {
      mounted = false;
      if (hlsRef.current) {
        hlsRef.current.destroy();
        hlsRef.current = null;
      }
    };
  }, [channel, activeLine, currentUrl]);

  // Handle play/pause
  const togglePlay = () => {
    const video = videoRef.current;
    if (!video) return;

    if (video.paused) {
      video
        .play()
        .then(() => {
          setIsPlaying(true);
          setPulseIcon('play');
          setPlayPulse(true);
          setTimeout(() => setPlayPulse(false), 500);
        })
        .catch(() => {});
    } else {
      video.pause();
      setIsPlaying(false);
      setPulseIcon('pause');
      setPlayPulse(true);
      setTimeout(() => setPlayPulse(false), 500);
    }
  };

  // Handle volume & mute
  const toggleMute = () => {
    const video = videoRef.current;
    if (!video) return;
    const nextMuted = !video.muted;
    video.muted = nextMuted;
    setIsMuted(nextMuted);
    try {
      localStorage.setItem('touchiptv-muted', String(nextMuted));
    } catch {}
  };

  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = parseFloat(e.target.value);
    setVolume(val);
    const video = videoRef.current;
    if (video) {
      video.volume = val;
      video.muted = val === 0;
      setIsMuted(val === 0);
    }
    try {
      localStorage.setItem('touchiptv-volume', String(val));
      localStorage.setItem('touchiptv-muted', String(val === 0));
    } catch {}
  };

  // Picture in Picture
  const togglePiP = async () => {
    const video = videoRef.current;
    if (!video) return;
    try {
      if (document.pictureInPictureElement) {
        await document.exitPictureInPicture();
      } else if (document.pictureInPictureEnabled) {
        await video.requestPictureInPicture();
      }
    } catch {}
  };

  const latencyDisplay = liveSpeedResult?.latency ?? channel?.latency;
  const speed = formatLatency(latencyDisplay);
  const resBadge = liveSpeedResult?.resolution || detectedResolution || channel?.resolution;

  return (
    <div className={`flex flex-col transition-all duration-300 ${isTheaterMode ? 'w-full' : 'w-full'}`}>
      {/* YouTube Style Player Viewport Container */}
      <div
        ref={playerFrameRef}
        onMouseMove={wakeControls}
        onTouchStart={wakeControls}
        onMouseLeave={() => isPlaying && setShowControls(false)}
        className={`group relative w-full aspect-video bg-black rounded-2xl overflow-hidden flex items-center justify-center select-none transition-colors ${
          isLight
            ? 'border border-slate-300/80 shadow-lg'
            : 'border border-[#2c323b] shadow-2xl'
        } ${isFullscreen ? 'fixed inset-0 z-50 rounded-none border-none' : ''}`}
      >
        {/* HTML5 Video Element */}
        <video
          ref={videoRef}
          playsInline
          // eslint-disable-next-line @typescript-eslint/ban-ts-comment
          {...{ 'webkit-playsinline': 'true' }}
          preload="auto"
          className="w-full h-full object-contain cursor-pointer"
          onClick={togglePlay}
          onDoubleClick={() => onToggleFullscreen()}
          onPlay={() => setIsPlaying(true)}
          onPause={() => setIsPlaying(false)}
          onPlaying={() => setLoading(false)}
          onWaiting={() => setLoading(true)}
        />

        {/* Center Tap-to-Play Overlay for Tablets if Autoplay is Blocked or Paused */}
        {!isPlaying && !loading && !errorMessage && channel && (
          <button
            type="button"
            onClick={togglePlay}
            className="absolute inset-0 m-auto w-16 h-16 rounded-2xl bg-sky-600/90 hover:bg-sky-500 active:scale-95 text-white flex items-center justify-center shadow-2xl backdrop-blur-md transition cursor-pointer z-25 border border-sky-400/50"
            aria-label="轻触开始播放"
            title="轻触开始播放"
          >
            <Play className="w-8 h-8 fill-current ml-1" />
          </button>
        )}

        {/* Center Click Pulse Animation (YouTube Style) */}
        {playPulse && (
          <div className="absolute inset-0 flex items-center justify-center pointer-events-none z-30 animate-ping duration-300">
            <div className="w-20 h-20 rounded-full bg-sky-500/80 flex items-center justify-center text-white shadow-2xl backdrop-blur-md">
              {pulseIcon === 'play' ? (
                <Play className="w-10 h-10 fill-current ml-1" />
              ) : (
                <Pause className="w-10 h-10 fill-current" />
              )}
            </div>
          </div>
        )}

        {/* Dedicated Exit Fullscreen Button */}
        {isFullscreen && (
          <button
            type="button"
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onToggleFullscreen(true);
            }}
            className="absolute top-5 right-5 z-50 flex items-center gap-2 px-5 py-2.5 rounded-full bg-rose-600/95 hover:bg-rose-500 active:scale-95 text-white font-medium shadow-2xl border border-rose-400 backdrop-blur-md transition cursor-pointer text-sm"
            aria-label="退出全屏"
          >
            <Minimize2 className="w-4 h-4" />
            <span>✕ 退出全屏</span>
          </button>
        )}

        {/* Top Floating Channel Info (Visible when controls shown) */}
        <div
          className={`absolute top-0 inset-x-0 p-4 bg-gradient-to-b from-black/85 via-black/40 to-transparent flex items-center justify-between text-white z-20 pointer-events-none transition-opacity duration-300 ${
            showControls || !isPlaying ? 'opacity-100' : 'opacity-0'
          }`}
        >
          <div className="flex items-center gap-2.5">
            <span className="w-2.5 h-2.5 rounded-full bg-emerald-400 animate-pulse" />
            <span className="font-bold text-sm tracking-wide text-slate-100 drop-shadow-sm">
              {channel ? channel.name : 'TouchIPTV 直播'}
            </span>
            {resBadge && (
              <span className="px-2 py-0.5 rounded text-[11px] font-bold bg-sky-500/25 border border-sky-400/40 text-sky-200">
                {resBadge}
              </span>
            )}
            {latencyDisplay !== undefined && (
              <span className="px-2 py-0.5 rounded text-[11px] font-mono bg-slate-900/80 border border-slate-700 text-emerald-300">
                {speed.text}
              </span>
            )}
          </div>
        </div>

        {/* Loading Spinner */}
        {loading && (
          <div className="absolute inset-0 flex flex-col items-center justify-center bg-black/60 backdrop-blur-xs text-white z-20 pointer-events-none">
            <div className="w-12 h-12 border-4 border-sky-400 border-t-transparent rounded-full animate-spin mb-3" />
            <p className="text-xs font-semibold tracking-wider text-sky-200">
              正在缓冲直播流 ({resBadge || '360-720P'})...
            </p>
          </div>
        )}

        {/* Error State */}
        {errorMessage && (
          <div
            className={`absolute inset-0 flex flex-col items-center justify-center z-20 p-6 text-center ${
              isLight ? 'bg-slate-900/95 text-white' : 'bg-[#14171a]/95 text-white'
            }`}
          >
            <AlertCircle className="w-12 h-12 text-amber-400 mb-3" />
            <h3 className="text-base font-bold text-slate-100 mb-1">直播流连接受阻</h3>
            <p className="text-xs text-slate-300 max-w-md mb-5 leading-relaxed">{errorMessage}</p>
            <div className="flex flex-wrap gap-2.5 justify-center">
              {channel && channel.lines && channel.lines.length > 1 && (
                <button
                  type="button"
                  onClick={() => onLineChange((activeLine + 1) % channel.lines.length)}
                  className="px-4 py-2 bg-sky-600 hover:bg-sky-500 text-white text-xs font-semibold rounded-xl flex items-center gap-1.5 transition cursor-pointer shadow-md shadow-sky-950/40"
                >
                  <Shuffle className="w-4 h-4" />
                  切换至线路 {((activeLine + 1) % channel.lines.length) + 1}
                </button>
              )}
              <button
                type="button"
                onClick={onRetry}
                className="px-4 py-2 bg-slate-800 hover:bg-slate-700 text-white text-xs font-semibold rounded-xl flex items-center gap-1.5 transition cursor-pointer border border-slate-700"
              >
                <RotateCcw className="w-4 h-4" />
                重试播放
              </button>
            </div>
          </div>
        )}

        {/* Empty State */}
        {!channel && (
          <div
            className={`absolute inset-0 flex flex-col items-center justify-center p-8 text-center z-10 ${
              isLight
                ? 'bg-gradient-to-b from-slate-100 to-white text-slate-700'
                : 'bg-gradient-to-b from-[#181a1d] to-[#121417] text-slate-300'
            }`}
          >
            <div className="w-16 h-16 rounded-2xl bg-sky-500/10 border border-sky-500/20 flex items-center justify-center text-sky-500 mb-4 shadow-lg">
              <Tv className="w-8 h-8" />
            </div>
            <h2 className={`text-xl font-bold mb-2 ${isLight ? 'text-slate-900' : 'text-white'}`}>
              轻享电视 · 极速直播
            </h2>
            <p className={`text-xs max-w-md mb-2 leading-relaxed ${isLight ? 'text-slate-500' : 'text-slate-400'}`}>
              已载入开源组合 104 个全频道直播流。请在左侧频道列表选择频道开始播放，支持分类筛选、剧场模式与双风格切换。
            </p>
          </div>
        )}

        {/* YouTube Style Bottom Floating Control Bar */}
        <div
          className={`absolute bottom-0 inset-x-0 p-3 bg-gradient-to-t from-black/95 via-black/60 to-transparent flex flex-col gap-1.5 z-30 transition-opacity duration-300 ${
            showControls || !isPlaying ? 'opacity-100' : 'opacity-0 pointer-events-none'
          }`}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="flex items-center justify-between gap-3 text-white">
            {/* Left Controls: Play/Pause, Channel Prev/Next, Volume, Live Badge */}
            <div className="flex items-center gap-2 sm:gap-3">
              {/* Play / Pause Toggle */}
              <button
                type="button"
                onClick={togglePlay}
                className="p-2 sm:p-2.5 rounded-xl hover:bg-white/20 active:scale-95 transition text-white cursor-pointer"
                aria-label={isPlaying ? '暂停' : '播放'}
                title={isPlaying ? '暂停 (空格)' : '播放 (空格)'}
              >
                {isPlaying ? <Pause className="w-5 h-5 fill-current" /> : <Play className="w-5 h-5 fill-current" />}
              </button>

              {/* Prev Channel */}
              {onPrevChannel && (
                <button
                  type="button"
                  onClick={onPrevChannel}
                  className="p-2 rounded-xl hover:bg-white/20 active:scale-95 transition text-white cursor-pointer hidden sm:block"
                  aria-label="上一个频道"
                  title="上一个频道 (方向上键)"
                >
                  <SkipBack className="w-4 h-4 fill-current" />
                </button>
              )}

              {/* Next Channel */}
              {onNextChannel && (
                <button
                  type="button"
                  onClick={onNextChannel}
                  className="p-2 rounded-xl hover:bg-white/20 active:scale-95 transition text-white cursor-pointer hidden sm:block"
                  aria-label="下一个频道"
                  title="下一个频道 (方向下键)"
                >
                  <SkipForward className="w-4 h-4 fill-current" />
                </button>
              )}

              {/* Volume & Mute */}
              <div className="flex items-center gap-1 group/vol">
                <button
                  type="button"
                  onClick={toggleMute}
                  className="p-2 rounded-xl hover:bg-white/20 active:scale-95 transition text-white cursor-pointer"
                  aria-label={isMuted ? '取消静音' : '静音'}
                  title={isMuted ? '取消静音' : '静音'}
                >
                  {isMuted || volume === 0 ? (
                    <VolumeX className="w-4 h-4 text-rose-400" />
                  ) : volume < 0.5 ? (
                    <Volume1 className="w-4 h-4" />
                  ) : (
                    <Volume2 className="w-4 h-4" />
                  )}
                </button>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  value={isMuted ? 0 : volume}
                  onChange={handleVolumeChange}
                  className="w-14 sm:w-20 h-1 bg-white/30 rounded-lg appearance-none cursor-pointer accent-sky-400 transition"
                  aria-label="音量调节"
                />
              </div>

              {/* LIVE Indicator */}
              <div className="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-rose-600/30 border border-rose-500/40 text-rose-300 text-[11px] font-bold">
                <Radio className="w-3 h-3 text-rose-400 animate-pulse" />
                <span>LIVE 直播</span>
              </div>
            </div>

            {/* Right Controls: Line Switcher, Speed Test, PiP, Theater Expand/Collapse, Fullscreen */}
            <div className="flex items-center gap-1.5 sm:gap-2">
              {/* Line Switcher inside video */}
              {channel && channel.lines && channel.lines.length > 1 && (
                <button
                  type="button"
                  onClick={() => onLineChange((activeLine + 1) % channel.lines.length)}
                  className="px-2.5 py-1.5 rounded-xl bg-slate-900/80 hover:bg-slate-800 border border-slate-700/80 text-sky-300 text-xs font-semibold flex items-center gap-1.5 transition active:scale-95 cursor-pointer"
                  title="切换备用源线路"
                >
                  <Shuffle className="w-3.5 h-3.5" />
                  <span>
                    {activeLine + 1}/{channel.lines.length}
                  </span>
                </button>
              )}

              {/* Speed Test Button */}
              {channel && (
                <button
                  type="button"
                  disabled={speedTesting}
                  onClick={() => onTestSpeed(channel)}
                  className="p-2 rounded-xl hover:bg-white/20 active:scale-95 transition text-sky-400 cursor-pointer disabled:opacity-50"
                  title="测速当前直播源"
                >
                  <Zap className={`w-4 h-4 ${speedTesting ? 'animate-bounce text-amber-400' : ''}`} />
                </button>
              )}

              {/* Picture in Picture */}
              <button
                type="button"
                onClick={togglePiP}
                className="p-2 rounded-xl hover:bg-white/20 active:scale-95 transition text-white cursor-pointer hidden sm:block"
                title="画中画模式"
                aria-label="画中画"
              >
                <PictureInPicture2 className="w-4 h-4" />
              </button>

              {/* YouTube Theater Mode (扩大 / 缩小) */}
              <button
                type="button"
                onClick={onToggleTheaterMode}
                className={`p-2 rounded-xl active:scale-95 transition cursor-pointer ${
                  isTheaterMode
                    ? 'bg-sky-600/40 text-sky-300 border border-sky-400/50'
                    : 'hover:bg-white/20 text-white'
                }`}
                title={isTheaterMode ? '缩小还原为标准视图 (T)' : '扩大为剧场宽屏视图 (T)'}
                aria-label={isTheaterMode ? '还原标准视图' : '扩大为剧场模式'}
              >
                {isTheaterMode ? <Minimize className="w-4 h-4" /> : <Maximize className="w-4 h-4" />}
              </button>

              {/* Fullscreen */}
              <button
                type="button"
                onClick={() => onToggleFullscreen()}
                className="p-2 rounded-xl hover:bg-white/20 active:scale-95 transition text-white cursor-pointer"
                title={isFullscreen ? '退出全屏 (F)' : '全屏播放 (F)'}
                aria-label={isFullscreen ? '退出全屏' : '全屏'}
              >
                {isFullscreen ? <Minimize2 className="w-4 h-4" /> : <Maximize2 className="w-4 h-4" />}
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* Under-Stage YouTube Style Metadata & Channel Bar (Adapts to 素雅灰白 / 铁灰) */}
      <div
        className={`mt-4 p-4 rounded-2xl border flex flex-col md:flex-row md:items-center justify-between gap-4 transition-colors duration-200 ${
          isLight
            ? 'bg-white border-slate-200 text-slate-800 shadow-sm'
            : 'bg-[#1e2227] border-[#2c323b] text-slate-100 shadow-xl'
        }`}
      >
        {/* Left: Channel Info, Multi-line selector pills */}
        <div className="flex items-start md:items-center gap-3">
          <div className="w-12 h-12 rounded-xl bg-gradient-to-tr from-sky-600 to-cyan-500 flex items-center justify-center text-white font-bold text-base shadow-md shadow-sky-950/30 shrink-0">
            {channel?.logo ? (
              <img
                src={channel.logo}
                alt=""
                className="w-full h-full object-contain rounded-xl p-1"
                onError={(e) => {
                  (e.target as HTMLElement).style.display = 'none';
                }}
              />
            ) : (
              <span>TV</span>
            )}
          </div>

          <div>
            <div className="flex items-center gap-2 flex-wrap">
              <h1 className={`text-lg md:text-xl font-bold tracking-tight ${isLight ? 'text-slate-900' : 'text-white'}`}>
                {channel ? channel.name : '请选择频道'}
              </h1>

              {/* Resolution Pill */}
              {resBadge && (
                <span
                  className={`px-2 py-0.5 rounded-md text-xs font-bold border ${
                    isLight
                      ? 'bg-sky-50 border-sky-200 text-sky-700'
                      : 'bg-sky-500/15 border-sky-500/30 text-sky-300'
                  }`}
                >
                  {resBadge}
                </span>
              )}

              {/* Latency Pill */}
              {latencyDisplay !== undefined && (
                <span
                  className={`px-2 py-0.5 rounded-md text-xs font-mono font-medium flex items-center gap-1 border ${
                    speed.status === 'fast'
                      ? isLight
                        ? 'bg-emerald-50 border-emerald-200 text-emerald-700'
                        : 'bg-emerald-500/15 border-emerald-500/30 text-emerald-300'
                      : speed.status === 'medium'
                      ? isLight
                        ? 'bg-amber-50 border-amber-200 text-amber-700'
                        : 'bg-amber-500/15 border-amber-500/30 text-amber-300'
                      : isLight
                      ? 'bg-rose-50 border-rose-200 text-rose-700'
                      : 'bg-rose-500/15 border-rose-500/30 text-rose-300'
                  }`}
                  title="实测网络延迟"
                >
                  <Zap className="w-3 h-3" />
                  {speed.text}
                </span>
              )}

              {channel?.group && (
                <span
                  className={`px-2 py-0.5 rounded-md text-xs border ${
                    isLight
                      ? 'bg-slate-100 text-slate-600 border-slate-200'
                      : 'bg-[#14171a] text-slate-400 border-[#2b313a]'
                  }`}
                >
                  {channel.group}
                </span>
              )}
            </div>

            {/* Direct Multi-line Selector Pills */}
            <div className="flex items-center gap-1.5 mt-2 flex-wrap">
              <span className={`text-[11px] mr-1 ${isLight ? 'text-slate-500' : 'text-slate-400'}`}>线路选择:</span>
              {channel && channel.lines && channel.lines.length > 0 ? (
                channel.lines.map((_, idx) => (
                  <button
                    key={idx}
                    type="button"
                    onClick={() => onLineChange(idx)}
                    className={`px-2.5 py-1 rounded-lg text-xs font-medium transition cursor-pointer ${
                      activeLine === idx
                        ? 'bg-sky-600 text-white font-bold shadow-xs'
                        : isLight
                        ? 'bg-slate-100 text-slate-600 hover:text-slate-900 hover:bg-slate-200 border border-slate-200'
                        : 'bg-[#14171a] text-slate-400 hover:text-white hover:bg-[#262b32] border border-[#2b313a]'
                    }`}
                  >
                    线路 {idx + 1}
                  </button>
                ))
              ) : (
                <span className={`text-xs ${isLight ? 'text-slate-400' : 'text-slate-500'}`}>默认线路</span>
              )}
            </div>
          </div>
        </div>

        {/* Right: Action Buttons with Unified Icon System & Playback Style Switcher */}
        <div className="flex items-center gap-2 flex-wrap">
          {/* Quick Style Switcher segmented control (素雅灰白 / 铁灰) */}
          <div
            className={`p-1 rounded-xl border flex items-center gap-1 ${
              isLight ? 'bg-slate-100 border-slate-200' : 'bg-[#14171a] border-[#2b313a]'
            }`}
            title="切换播放界面风格：素雅灰白 / 铁灰"
          >
            <button
              type="button"
              onClick={() => onThemeChange('light')}
              className={`px-2.5 py-1 rounded-lg text-xs font-medium flex items-center gap-1 transition cursor-pointer ${
                isLight
                  ? 'bg-white text-slate-900 font-bold shadow-xs'
                  : 'text-slate-400 hover:text-slate-200'
              }`}
            >
              <Sun className={`w-3.5 h-3.5 ${isLight ? 'text-amber-600' : 'text-slate-400'}`} />
              <span>素雅灰白</span>
            </button>
            <button
              type="button"
              onClick={() => onThemeChange('iron')}
              className={`px-2.5 py-1 rounded-lg text-xs font-medium flex items-center gap-1 transition cursor-pointer ${
                !isLight
                  ? 'bg-[#242930] text-sky-300 font-bold shadow-xs border border-[#363d48]'
                  : 'text-slate-600 hover:text-slate-900'
              }`}
            >
              <Moon className={`w-3.5 h-3.5 ${!isLight ? 'text-sky-400' : 'text-slate-500'}`} />
              <span>铁灰</span>
            </button>
          </div>

          {/* Favorite */}
          {channel && (
            <button
              type="button"
              onClick={() => onToggleFavorite(channel.name)}
              className={`px-3 py-2 rounded-xl text-xs font-semibold border flex items-center gap-1.5 transition active:scale-95 cursor-pointer ${
                isFavorite
                  ? isLight
                    ? 'bg-amber-50 border-amber-300 text-amber-800'
                    : 'bg-amber-500/15 border-amber-500/40 text-amber-300'
                  : isLight
                  ? 'bg-slate-100 hover:bg-slate-200 border-slate-200 text-slate-700'
                  : 'bg-[#242930] hover:bg-[#2c323b] border-[#343b46] text-slate-300'
              }`}
            >
              <Star className={`w-3.5 h-3.5 ${isFavorite ? 'fill-amber-400 text-amber-400' : ''}`} />
              <span>{isFavorite ? '已收藏' : '收藏'}</span>
            </button>
          )}

          {/* Quick Retry */}
          {channel && (
            <button
              type="button"
              onClick={onRetry}
              className={`px-3 py-2 rounded-xl text-xs font-semibold border flex items-center gap-1.5 transition active:scale-95 cursor-pointer ${
                isLight
                  ? 'bg-slate-100 hover:bg-slate-200 border-slate-200 text-slate-700'
                  : 'bg-[#242930] hover:bg-[#2c323b] border-[#343b46] text-slate-300'
              }`}
              title="重新加载当前线路"
            >
              <RotateCcw className="w-3.5 h-3.5" />
              <span>刷新线路</span>
            </button>
          )}

          {/* Theater Mode Toggle Button (扩大 / 缩小) */}
          <button
            type="button"
            onClick={onToggleTheaterMode}
            className={`px-3 py-2 rounded-xl text-xs font-semibold border flex items-center gap-1.5 transition active:scale-95 cursor-pointer ${
              isTheaterMode
                ? isLight
                  ? 'bg-sky-50 text-sky-700 border-sky-300 font-bold'
                  : 'bg-sky-600 text-white border-sky-500 shadow-md'
                : isLight
                ? 'bg-slate-100 hover:bg-slate-200 border-slate-200 text-slate-700'
                : 'bg-[#242930] hover:bg-[#2c323b] border-[#343b46] text-slate-300'
            }`}
            title="扩大为剧场宽屏或缩小为标准视图 (T)"
          >
            {isTheaterMode ? <Minimize className="w-3.5 h-3.5" /> : <Maximize className="w-3.5 h-3.5" />}
            <span>{isTheaterMode ? '缩小还原' : '扩大视图'}</span>
          </button>

          {/* Fullscreen Button */}
          <button
            type="button"
            onClick={() => onToggleFullscreen()}
            className="px-3.5 py-2 rounded-xl text-xs font-semibold bg-sky-600 hover:bg-sky-500 text-white active:scale-95 flex items-center gap-1.5 shadow-sm transition cursor-pointer"
          >
            {isFullscreen ? <Minimize2 className="w-3.5 h-3.5" /> : <Maximize2 className="w-3.5 h-3.5" />}
            <span>{isFullscreen ? '退出全屏' : '全屏'}</span>
          </button>
        </div>
      </div>

      {/* Notice info banner */}
      {notice && (
        <div
          className={`mt-2.5 px-3 py-1.5 rounded-xl text-xs border flex items-center justify-between transition-colors ${
            isLight
              ? 'bg-white/80 border-slate-200 text-slate-600'
              : 'bg-[#14171a]/80 border-[#282e36] text-slate-400'
          }`}
        >
          <div className="flex items-center gap-2 truncate">
            <span className="w-1.5 h-1.5 rounded-full bg-sky-500 shrink-0" />
            <span className="truncate">{notice}</span>
          </div>
        </div>
      )}
    </div>
  );
};
