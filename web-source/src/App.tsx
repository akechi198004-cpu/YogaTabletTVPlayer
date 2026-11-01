import React, { useState, useEffect, useRef, useMemo, useDeferredValue } from 'react';
import defaultChannelsData from './data/defaultChannels.json';
import { ChannelItem, filterChannels } from './utils/m3uParser';
import {
  saveChannelsToStorage,
  loadChannelsFromStorage,
  clearChannelsStorage,
} from './utils/channelStorage';
import { Player } from './components/Player';
import { Sidebar } from './components/Sidebar';
import { TopBar } from './components/TopBar';

const STORAGE_KEY = 'touchiptv-v3';
const THEME_STORAGE_KEY = 'touchiptv-theme';

export type ThemeMode = 'iron' | 'light';

const FILTER_GROUPS = [
  '全部',
  '收藏',
  '央视',
  '卫视',
  '苏州',
  '香港',
  '台湾',
  '日本',
  '国际',
  '纪录',
  '少儿',
];

interface StoredData {
  fav: string[];
  last: string;
  filter: string;
}

export default function App() {
  const [channels, setChannels] = useState<ChannelItem[]>(() => defaultChannelsData as ChannelItem[]);
  const [currentChannel, setCurrentChannel] = useState<ChannelItem | null>(() => {
    const list = defaultChannelsData as ChannelItem[];
    return list.find((c) => c.name.includes('CCTV-1')) || list[0] || null;
  });
  const [activeLine, setActiveLine] = useState<number>(0);
  const [activeFilter, setActiveFilter] = useState<string>('全部');
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [favorites, setFavorites] = useState<string[]>([]);
  const [sidebarOpen, setSidebarOpen] = useState<boolean>(false);
  const [isFullscreen, setIsFullscreen] = useState<boolean>(false);
  const [isTheaterMode, setIsTheaterMode] = useState<boolean>(false);
  const [notice, setNotice] = useState<string>('已就绪：已载入开源组合 104 个全频道直播流。');
  const [loadingChannels, setLoadingChannels] = useState<boolean>(false);

  // Theme style state: 'iron' (铁灰) | 'light' (素雅灰白)
  const [theme, setTheme] = useState<ThemeMode>(() => {
    try {
      const savedTheme = localStorage.getItem(THEME_STORAGE_KEY);
      if (savedTheme === 'light' || savedTheme === 'iron') return savedTheme;
    } catch {}
    return 'iron';
  });

  const handleThemeChange = (newTheme: ThemeMode) => {
    setTheme(newTheme);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, newTheme);
    } catch {}
  };

  // Speed test state
  const [speedTestingSingle, setSpeedTestingSingle] = useState<boolean>(false);
  const [liveSpeedResult, setLiveSpeedResult] = useState<{ latency?: number; resolution?: string } | null>(null);
  const [isTestingAll, setIsTestingAll] = useState<boolean>(false);
  const [batchProgressText, setBatchProgressText] = useState<string>('');

  const playerFrameRef = useRef<HTMLDivElement | null>(null);

  // Initialize stored preferences and cached channels
  useEffect(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const parsed: Partial<StoredData> = JSON.parse(raw);
        if (Array.isArray(parsed.fav)) setFavorites(parsed.fav);
        if (parsed.filter && FILTER_GROUPS.includes(parsed.filter)) {
          setActiveFilter(parsed.filter);
        }
        if (parsed.last) {
          const matched = (defaultChannelsData as ChannelItem[]).find((c) => c.name === parsed.last);
          if (matched) setCurrentChannel(matched);
        }
      }
    } catch {}

    // Load cached speed test metrics from IndexedDB if available
    loadChannelsFromStorage().then((cached) => {
      if (cached && Array.isArray(cached) && cached.length === defaultChannelsData.length) {
        setChannels(cached);
        if (!currentChannel) {
          setCurrentChannel(cached[0]);
        }
      }
    });
  }, []);

  // Save changes to storage
  const persistState = (newFavs: string[], lastChan: string, filterStr: string) => {
    try {
      const data: StoredData = {
        fav: newFavs,
        last: lastChan,
        filter: filterStr,
      };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
    } catch {}
  };

  // Toggle favorite
  const handleToggleFavorite = (name: string) => {
    const next = favorites.includes(name) ? favorites.filter((x) => x !== name) : [...favorites, name];
    setFavorites(next);
    persistState(next, currentChannel?.name || '', activeFilter);
  };

  // Channel select
  const handleSelectChannel = (channel: ChannelItem) => {
    setCurrentChannel(channel);
    setActiveLine(0);
    setLiveSpeedResult(null);
    setNotice(`正在连接频道：${channel.name} (${channel.resolution || '高清'}) · 线路 1`);
    persistState(favorites, channel.name, activeFilter);
    if (window.innerWidth < 1024) {
      setSidebarOpen(false);
    }
  };

  // Deferred search query for 60fps typing responsiveness
  const deferredSearchQuery = useDeferredValue(searchQuery);

  // Filtered channels
  const filteredChannels = useMemo(() => {
    return filterChannels(channels, deferredSearchQuery, activeFilter, favorites);
  }, [channels, deferredSearchQuery, activeFilter, favorites]);

  // Fullscreen toggle with fallback support
  const handleToggleFullscreen = async (forceExit: boolean = false) => {
    const frame = playerFrameRef.current;
    if (!frame) return;

    if (document.fullscreenElement) {
      try {
        await document.exitFullscreen();
        setIsFullscreen(false);
      } catch {
        setIsFullscreen(false);
      }
    } else if (frame.classList.contains('fullscreen-fallback')) {
      frame.classList.remove('fullscreen-fallback');
      document.body.classList.remove('overflow-hidden');
      setIsFullscreen(false);
    } else if (!forceExit) {
      try {
        if (frame.requestFullscreen) {
          await frame.requestFullscreen();
          setIsFullscreen(true);
        } else {
          throw new Error('Not supported');
        }
      } catch {
        frame.classList.add('fullscreen-fallback');
        document.body.classList.add('overflow-hidden');
        setIsFullscreen(true);
      }
    }
  };

  useEffect(() => {
    const onFsChange = () => {
      setIsFullscreen(Boolean(document.fullscreenElement));
    };
    document.addEventListener('fullscreenchange', onFsChange);
    document.addEventListener('webkitfullscreenchange', onFsChange);

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setSidebarOpen(false);
        if (isFullscreen) handleToggleFullscreen(true);
      }
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) {
        return;
      }
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        if (filteredChannels.length === 0) return;
        const curIdx = filteredChannels.findIndex((c) => c.name === currentChannel?.name);
        const nextIdx =
          e.key === 'ArrowDown'
            ? (curIdx + 1) % filteredChannels.length
            : (curIdx - 1 + filteredChannels.length) % filteredChannels.length;
        handleSelectChannel(filteredChannels[nextIdx]);
      } else if (e.key === 't' || e.key === 'T') {
        setIsTheaterMode((prev) => !prev);
      } else if (e.key === 'f' || e.key === 'F') {
        handleToggleFullscreen();
      } else if (e.key === 's' || e.key === 'S') {
        setSidebarOpen((prev) => !prev);
      } else if (e.key === '[' || e.key === ']') {
        e.preventDefault();
        if (filteredChannels.length === 0) return;
        const curIdx = filteredChannels.findIndex((c) => c.name === currentChannel?.name);
        const nextIdx =
          e.key === ']'
            ? (curIdx + 1) % filteredChannels.length
            : (curIdx - 1 + filteredChannels.length) % filteredChannels.length;
        handleSelectChannel(filteredChannels[nextIdx]);
      } else if (['1', '2', '3', '4', '5'].includes(e.key)) {
        const lineIdx = parseInt(e.key, 10) - 1;
        if (currentChannel && currentChannel.lines && lineIdx < currentChannel.lines.length) {
          setActiveLine(lineIdx);
          setNotice(`已通过数字键 ${e.key} 切换至：线路 ${lineIdx + 1}`);
        }
      }
    };
    window.addEventListener('keydown', onKeyDown);

    return () => {
      document.removeEventListener('fullscreenchange', onFsChange);
      document.removeEventListener('webkitfullscreenchange', onFsChange);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [filteredChannels, currentChannel, isFullscreen]);

  // Speed test for single active channel
  const handleTestSpeed = async (channel: ChannelItem) => {
    const testUrl = channel.lines?.[activeLine] || channel.url;
    if (!testUrl) return;

    setSpeedTestingSingle(true);
    setNotice(`正在测速：${channel.name}...`);
    try {
      const res = await fetch(`/api/speedtest?url=${encodeURIComponent(testUrl)}`);
      if (res.ok) {
        const data = await res.json();
        if (data.ok) {
          setLiveSpeedResult({ latency: data.latency, resolution: data.resolution });
          setChannels((prev) => {
            const nextList = prev.map((c) =>
              c.name === channel.name
                ? {
                    ...c,
                    latency: data.latency,
                    resolution: data.resolution || c.resolution,
                    height: data.height || c.height,
                  }
                : c
            );
            saveChannelsToStorage(nextList);
            return nextList;
          });
          setNotice(`测速完成：${channel.name} 延迟 ${data.latency}ms · 画质 ${data.resolution} (连接正常)`);
        } else {
          setNotice(`测速提示：${channel.name} 测速连接失败 (${data.error || '超时'})`);
        }
      }
    } catch {
      setNotice(`测速错误：无法连通测速接口`);
    } finally {
      setSpeedTestingSingle(false);
    }
  };

  // Batch speed test for visible channels (3 workers from top to bottom)
  const handleBatchSpeedTest = async () => {
    if (isTestingAll || filteredChannels.length === 0) return;
    setIsTestingAll(true);
    setNotice(`开始对当前列表中的 ${filteredChannels.length} 个频道并发测速...`);

    const queue = [...filteredChannels];
    let completed = 0;
    const CONCURRENCY = 3;

    const worker = async () => {
      while (queue.length > 0) {
        const item = queue.shift();
        if (!item) break;
        const testUrl = item.lines?.[0] || item.url;
        try {
          const res = await fetch(`/api/speedtest?url=${encodeURIComponent(testUrl)}`);
          if (res.ok) {
            const data = await res.json();
            if (data.ok) {
              setChannels((prev) =>
                prev.map((c) =>
                  c.name === item.name
                    ? {
                        ...c,
                        latency: data.latency,
                        resolution: data.resolution || c.resolution,
                        height: data.height || c.height,
                      }
                    : c
                )
              );
            }
          }
        } catch {}
        completed++;
        setBatchProgressText(`${completed}/${filteredChannels.length}`);
      }
    };

    await Promise.all(Array.from({ length: CONCURRENCY }, () => worker()));
    setIsTestingAll(false);
    setChannels((latest) => {
      saveChannelsToStorage(latest);
      return latest;
    });
    setNotice(`批量测速已完成：已更新当前所有频道的实时延迟与画质信息。`);
  };

  // Reset & refresh channels list from default data
  const handleRefresh = async () => {
    setLoadingChannels(true);
    setNotice(`正在刷新频道列表...`);
    try {
      const resetList = defaultChannelsData as ChannelItem[];
      setChannels(resetList);
      await clearChannelsStorage();
      await saveChannelsToStorage(resetList);
      setNotice(`已刷新频道列表：当前共 ${resetList.length} 个频道。`);
    } catch {
      setNotice('刷新频道列表遇到问题');
    } finally {
      setLoadingChannels(false);
    }
  };

  const isLight = theme === 'light';

  return (
    <div
      className={`flex h-screen w-screen overflow-hidden font-sans transition-colors duration-200 ${
        isLight ? 'bg-[#f4f5f7] text-slate-800' : 'bg-[#181a1d] text-slate-100'
      }`}
    >
      {/* Sidebar Navigation */}
      <Sidebar
        channels={channels}
        filteredChannels={filteredChannels}
        currentChannel={currentChannel}
        onSelectChannel={handleSelectChannel}
        activeFilter={activeFilter}
        onFilterChange={setActiveFilter}
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
        favorites={favorites}
        onToggleFavorite={handleToggleFavorite}
        isOpen={sidebarOpen}
        onClose={() => setSidebarOpen(false)}
        onRefresh={handleRefresh}
        loading={loadingChannels}
        filterGroups={FILTER_GROUPS}
        onBatchSpeedTest={handleBatchSpeedTest}
        isTestingAll={isTestingAll}
        testProgressText={batchProgressText}
        theme={theme}
      />

      {/* Main Content Area */}
      <div className="flex-1 flex flex-col min-w-0 overflow-y-auto">
        <TopBar
          onOpenSidebar={() => setSidebarOpen(true)}
          currentChannel={currentChannel}
          isFullscreen={isFullscreen}
          onToggleFullscreen={() => handleToggleFullscreen()}
          sourceStatusText={`共 ${filteredChannels.length} 频道`}
          onBatchSpeedTest={handleBatchSpeedTest}
          isTestingAll={isTestingAll}
          testProgressText={batchProgressText}
          theme={theme}
          onThemeChange={handleThemeChange}
        />

        <main
          className={`flex-1 p-3 sm:p-5 transition-all duration-300 w-full mx-auto flex flex-col justify-start ${
            isTheaterMode ? 'max-w-none px-2 sm:px-6' : 'max-w-6xl'
          }`}
        >
          <Player
            channel={currentChannel}
            activeLine={activeLine}
            onLineChange={setActiveLine}
            onRetry={() => {
              const cur = currentChannel;
              setCurrentChannel(null);
              setTimeout(() => setCurrentChannel(cur), 50);
            }}
            onToggleFavorite={handleToggleFavorite}
            isFavorite={Boolean(currentChannel && favorites.includes(currentChannel.name))}
            isFullscreen={isFullscreen}
            onToggleFullscreen={handleToggleFullscreen}
            isTheaterMode={isTheaterMode}
            onToggleTheaterMode={() => setIsTheaterMode((prev) => !prev)}
            onNextChannel={() => {
              if (filteredChannels.length === 0) return;
              const curIdx = filteredChannels.findIndex((c) => c.name === currentChannel?.name);
              const nextIdx = (curIdx + 1) % filteredChannels.length;
              handleSelectChannel(filteredChannels[nextIdx]);
            }}
            onPrevChannel={() => {
              if (filteredChannels.length === 0) return;
              const curIdx = filteredChannels.findIndex((c) => c.name === currentChannel?.name);
              const prevIdx = (curIdx - 1 + filteredChannels.length) % filteredChannels.length;
              handleSelectChannel(filteredChannels[prevIdx]);
            }}
            playerFrameRef={playerFrameRef}
            notice={notice}
            onTestSpeed={handleTestSpeed}
            speedTesting={speedTestingSingle}
            liveSpeedResult={liveSpeedResult}
            theme={theme}
            onThemeChange={handleThemeChange}
          />
        </main>
      </div>
    </div>
  );
}
